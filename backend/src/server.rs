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

const UPDATE_THROTTLE_MS: u64 = 1000;

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

    let current_metrics = metrics.compute_metrics(Some(&orderbook_manager)).await;
    let client_msg = ClientMessage::Metrics(current_metrics);
    let json = serde_json::to_string(&client_msg)?;
    client_ws_write.send(Message::Text(json.into())).await?;

    // Throttle book updates to avoid overwhelming clients
    let mut pending_book_updates: HashMap<String, ClientMessage> = HashMap::new();
    let mut throttle_ticker = interval(Duration::from_millis(UPDATE_THROTTLE_MS));
    throttle_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            // Send pending updates periodically (throttling)
            _ = throttle_ticker.tick() => {
                if !pending_book_updates.is_empty() {
                    for (_key, client_msg) in pending_book_updates.drain() {
                        if let Ok(json) = serde_json::to_string(&client_msg) {
                            if let Err(e) = client_ws_write.send(Message::Text(json.into())).await {
                                tracing::debug!("Failed to send throttled update to client {}: {}", client_addr, e);
                                break;
                            }
                        }
                    }
                }
            }

            // Receive updates from broadcast channel
            broadcast_result = client_broadcast_rx.recv() => {
                match broadcast_result {
                    Ok(client_msg) => {
                        match &client_msg {
                            ClientMessage::BookUpdate { exchange, symbol, .. } => {
                                // Throttle orderbook updates (keep only latest per exchange:symbol)
                                let key = format!("{}:{}", exchange, symbol);
                                pending_book_updates.insert(key, client_msg);
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
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Client {} lagged by {} messages, sending full snapshot", client_addr, n);
                        // Client fell behind - send current state for all symbols to catch up
                        for entry in orderbook_manager.iter() {
                            let book = entry.value();
                            if book.is_initialized() {
                                let client_msg = book.to_client_message(ORDERBOOK_DISPLAY_DEPTH);
                                let json = serde_json::to_string(&client_msg)?;
                                let _ = client_ws_write.send(Message::Text(json.into())).await;
                            }
                        }
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
