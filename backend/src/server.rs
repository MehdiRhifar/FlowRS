use crate::metrics::SharedMetrics;
use crate::orderbook::SharedOrderBook;
use crate::types::ClientMessage;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;

/// Start the WebSocket server for frontend clients
pub async fn start_server(
    addr: &str,
    orderbook: SharedOrderBook,
    metrics: SharedMetrics,
    tx: broadcast::Sender<ClientMessage>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("WebSocket server listening on {}", addr);

    while let Ok((stream, addr)) = listener.accept().await {
        let orderbook = orderbook.clone();
        let metrics = metrics.clone();
        let rx = tx.subscribe();

        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, addr, orderbook, metrics, rx).await {
                tracing::error!("Client {} error: {}", addr, e);
            }
        });
    }

    Ok(())
}

async fn handle_client(
    stream: TcpStream,
    addr: SocketAddr,
    orderbook: SharedOrderBook,
    metrics: SharedMetrics,
    mut rx: broadcast::Receiver<ClientMessage>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing::info!("New client connected: {}", addr);

    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    let (mut write, mut read) = ws_stream.split();

    // Send current order book state immediately
    {
        let book = orderbook.read().await;
        if book.is_initialized() {
            let msg = book.to_client_message(20);
            let json = serde_json::to_string(&msg)?;
            write.send(Message::Text(json.into())).await?;
        }
    }

    // Send current metrics
    {
        let current_metrics = metrics.get_cached_metrics().await;
        let msg = ClientMessage::Metrics(current_metrics);//
        let json = serde_json::to_string(&msg)?;
        write.send(Message::Text(json.into())).await?;
    }

    // Handle incoming messages and broadcast updates
    loop {
        tokio::select! {
            // Receive updates from broadcast channel
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        let json = serde_json::to_string(&msg)?;
                        if let Err(e) = write.send(Message::Text(json.into())).await {
                            tracing::debug!("Failed to send to client {}: {}", addr, e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Client {} lagged by {} messages", addr, n);
                        // Send current state to catch up
                        let book = orderbook.read().await;
                        if book.is_initialized() {
                            let msg = book.to_client_message(20);
                            let json = serde_json::to_string(&msg)?;
                            let _ = write.send(Message::Text(json.into())).await;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }

            // Handle client messages (ping/pong, close)
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) => {
                        tracing::info!("Client {} disconnected", addr);
                        break;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = write.send(Message::Pong(data)).await;
                    }
                    Some(Err(e)) => {
                        tracing::debug!("Client {} error: {}", addr, e);
                        break;
                    }
                    None => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    tracing::info!("Client {} handler finished", addr);
    Ok(())
}
