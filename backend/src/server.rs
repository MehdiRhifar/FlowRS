//! WebSocket server for frontend clients with per-client throttling

use crate::metrics::SharedMetrics;
use crate::orderbook::SharedOrderBookManager;
use crate::types::{ClientMessage, ORDERBOOK_DISPLAY_DEPTH, TRADING_PAIRS};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::time::{interval, Duration};
use tokio_tungstenite::tungstenite::Message;

const BOOK_POLL_MS: u64 = 200;

/// Start the WebSocket server for frontend clients
pub async fn start_server(
    addr: &str,
    orderbook_manager: SharedOrderBookManager,
    metrics: SharedMetrics,
    client_broadcast_tx: broadcast::Sender<ClientMessage>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("WebSocket server listening on {}", addr);

    while let Ok((client_stream, client_addr)) = listener.accept().await {
        // Clone shared state for this client
        let orderbook_manager = orderbook_manager.clone();
        let metrics = metrics.clone();
        let client_broadcast_rx = client_broadcast_tx.subscribe();

        metrics.increment_connections();

        // Spawn handler for this client
        tokio::spawn(async move {
            if let Err(e) = handle_client(
                client_stream,
                client_addr,
                orderbook_manager,
                metrics.clone(),
                client_broadcast_rx,
            )
            .await
            {
                tracing::error!("Client {} error: {}", client_addr, e);
            }
            metrics.decrement_connections();
        });
    }

    Ok(())
}

async fn handle_client(
    client_tcp_stream: TcpStream,
    client_addr: SocketAddr,
    orderbook_manager: SharedOrderBookManager,
    metrics: SharedMetrics,
    mut client_broadcast_rx: broadcast::Receiver<ClientMessage>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing::info!("New client connected: {}", client_addr);

    let client_ws_stream = tokio_tungstenite::accept_async(client_tcp_stream).await?;
    let (mut client_ws_write, mut client_ws_read) = client_ws_stream.split();

    // Send initial snapshot
    let symbols: Vec<String> = TRADING_PAIRS.iter().map(|s| s.to_string()).collect();
    let client_msg = ClientMessage::SymbolList(symbols);
    let json = serde_json::to_string(&client_msg)?;
    client_ws_write.send(Message::Text(json.into())).await?;

    for entry in orderbook_manager.iter() {
        let book = entry.value();
        if book.is_initialized() {
            let client_msg = book.to_client_message(ORDERBOOK_DISPLAY_DEPTH);
            let json = serde_json::to_string(&client_msg)?;
            client_ws_write.send(Message::Text(json.into())).await?;
        }
    }

    let current_metrics = metrics.compute_metrics();
    let client_msg = ClientMessage::Metrics(current_metrics);
    let json = serde_json::to_string(&client_msg)?;
    client_ws_write.send(Message::Text(json.into())).await?;

    // Track last sent update_id per orderbook to avoid redundant sends
    let mut last_sent_update_id: HashMap<String, u64> = HashMap::new();

    // Poll orderbooks periodically and send only if changed
    let mut book_poll_ticker = interval(Duration::from_millis(BOOK_POLL_MS));
    book_poll_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut messages_buffer = Vec::with_capacity(TRADING_PAIRS.len());
    loop {
        tokio::select! {
            // Poll orderbooks and send updates if changed
            _ = book_poll_ticker.tick() => {
                messages_buffer.clear();
                for entry in orderbook_manager.iter() {
                    let book = entry.value();

                    if !book.is_initialized() {
                        continue;
                    }

                    let key = entry.key().clone();
                    let current_update_id = book.last_update_id();

                    // Check if this orderbook has been updated since last send
                    let should_send = match last_sent_update_id.get(&key) {
                        Some(&last_id) => current_update_id != last_id,
                        None => true, // First time seeing this book
                    };

                    if should_send {
                        // On construit le message (copie mémoire)
                        let client_msg = book.to_client_message(ORDERBOOK_DISPLAY_DEPTH);

                        // On stocke le message et la clé pour mettre à jour l'ID après
                        messages_buffer.push((key, current_update_id, client_msg));
                    }
                }
                // PHASE 2: Envoi Réseau (Lent, Async, sans verrou)
                for (key, update_id, client_msg) in messages_buffer.drain(..) {
                    if let Ok(json) = serde_json::to_string(&client_msg) {
                        if let Err(e) = client_ws_write.send(Message::Text(json.into())).await {
                            tracing::debug!("Failed to send book update to client {}: {}", client_addr, e);
                            // Si le client est déconnecté, on arrête tout
                            return Ok(());
                        }
                        // On ne met à jour l'ID que si l'envoi a réussi
                        last_sent_update_id.insert(key, update_id);
                    }
                }
            }

            // Receive updates from broadcast channel (Trades and Metrics only)
            broadcast_result = client_broadcast_rx.recv() => {
                match broadcast_result {
                    Ok(client_msg) => {
                        match &client_msg {
                            ClientMessage::BookUpdate { .. } => {
                                // BookUpdates are no longer sent via broadcast - ignore
                            }
                            _ => {
                                // Send trades and metrics immediately (no throttling)
                                let json = serde_json::to_string(&client_msg)?;
                                if let Err(e) = client_ws_write.send(Message::Text(json.into())).await {
                                    tracing::debug!("Failed to send to client {}: {}", client_addr, e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_n)) => {
                        // Client lagged on Trades/Metrics - not critical, just skip
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::info!("Broadcast channel closed");
                        break;
                    }
                }
            }

            // Handle messages from client (ping/pong, close, etc.)
            client_ws_msg = client_ws_read.next() => {
                match client_ws_msg {
                    Some(Ok(Message::Close(_))) => {
                        tracing::info!("Client {} disconnected", client_addr);
                        break;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = client_ws_write.send(Message::Pong(data)).await;
                    }
                    Some(Err(e)) => {
                        tracing::debug!("Client {} WebSocket error: {}", client_addr, e);
                        break;
                    }
                    None => {
                        tracing::info!("Client {} connection closed", client_addr);
                        break;
                    }
                    _ => {
                        // Ignore other message types
                    }
                }
            }
        }
    }

    tracing::info!("Client {} handler finished", client_addr);
    Ok(())
}
