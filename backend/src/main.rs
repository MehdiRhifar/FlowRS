//! Real-time order book aggregator for cryptocurrency exchanges

mod exchanges;
mod metrics;
mod orderbook;
mod server;
mod types;

use crate::exchanges::{BinanceConn, BybitConn, ExchangeConnector, ExchangeManager};
use crate::metrics::create_shared_metrics;
use crate::orderbook::create_shared_orderbook_manager;
use crate::types::{ClientMessage, TRADING_PAIRS};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing_subscriber::EnvFilter;

const SERVER_ADDR: &str = "0.0.0.0:8080";
const BROADCAST_CAPACITY: usize = 4096; // Increased for multiple symbols

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting Order Book Visualizer Backend");
    tracing::info!("Tracking {} trading pairs: {:?}", TRADING_PAIRS.len(), TRADING_PAIRS);

    let orderbook_manager = create_shared_orderbook_manager();
    let metrics = create_shared_metrics();
    let (client_broadcast_tx, _) = broadcast::channel::<ClientMessage>(BROADCAST_CAPACITY);

    let symbols: Vec<String> = TRADING_PAIRS.iter().map(|s| s.to_string()).collect();
    let exchange_connectors = vec![
        ExchangeConnector::Binance(BinanceConn::new(symbols.clone())),
        ExchangeConnector::Bybit(BybitConn::new(symbols.clone())),
    ];

    tracing::info!("Configured {} exchange(s)", exchange_connectors.len());
    for connector in &exchange_connectors {
        tracing::info!("  • {}", connector.exchange().name());
    }

    let exchange_manager = ExchangeManager::new(
        exchange_connectors,
        orderbook_manager.clone(),
        metrics.clone(),
    );

    // Broadcast metrics every second
    let _metrics_ticker = {
        let orderbook_manager = orderbook_manager.clone();
        let metrics = metrics.clone();
        let broadcast_tx = client_broadcast_tx.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                let current_metrics = metrics.compute_metrics(Some(&orderbook_manager)).await;
                let _ = broadcast_tx.send(ClientMessage::Metrics(current_metrics));
            }
        })
    };

    let _exchange_handles = exchange_manager
        .start_all(client_broadcast_tx.clone())
        .await;

    tracing::info!("Starting WebSocket server on {}", SERVER_ADDR);
    server::start_server(
        SERVER_ADDR,
        orderbook_manager,
        metrics,
        client_broadcast_tx,
    )
    .await?;

    Ok(())
}
