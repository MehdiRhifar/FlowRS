mod binance;
mod metrics;
mod orderbook;
mod server;
mod types;

use crate::metrics::create_shared_metrics;
use crate::orderbook::create_shared_orderbook;
use crate::types::ClientMessage;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing_subscriber::EnvFilter;

const SERVER_ADDR: &str = "0.0.0.0:8080";
const BROADCAST_CAPACITY: usize = 1024;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting Order Book Visualizer Backend");

    // Create shared state
    let orderbook = create_shared_orderbook();
    let metrics = create_shared_metrics();

    // Create broadcast channel for client updates
    let (tx, _rx) = broadcast::channel::<ClientMessage>(BROADCAST_CAPACITY);

    // Clone for different tasks
    let orderbook_binance = orderbook.clone();
    let metrics_binance = metrics.clone();
    let tx_binance = tx.clone();

    let orderbook_server = orderbook.clone();
    let metrics_server = metrics.clone();
    let tx_server = tx.clone();

    let metrics_ticker = metrics.clone();
    let tx_metrics = tx.clone();

    // Start metrics ticker (sends metrics every second)
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let current_metrics = metrics_ticker.compute_metrics().await;
            let msg = ClientMessage::Metrics(current_metrics);
            let _ = tx_metrics.send(msg);
        }
    });

    // Start Binance connection (with automatic reconnection)
    tokio::spawn(async move {
        if let Err(e) =
            binance::start_binance_connection(orderbook_binance, metrics_binance, tx_binance).await
        {
            tracing::error!("Binance connection fatal error: {}", e);
        }
    });

    // Start WebSocket server
    tracing::info!("Starting WebSocket server on {}", SERVER_ADDR);
    server::start_server(SERVER_ADDR, orderbook_server, metrics_server, tx_server).await?;

    Ok(())
}
