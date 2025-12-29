//! Real-time order book aggregator for cryptocurrency exchanges

mod exchanges;
mod metrics;
mod orderbook;
mod server;
mod types;

use crate::exchanges::{
    BinanceConn, BybitConn, CoinbaseConn, ExchangeConnector, ExchangeManager, KrakenConn,
};
use crate::metrics::create_shared_metrics;
use crate::orderbook::create_shared_orderbook_manager;
use crate::types::{ClientMessage, TRADING_PAIRS};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing_subscriber::EnvFilter;

use tikv_jemallocator::Jemalloc;

#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

const SERVER_ADDR: &str = "0.0.0.0:8080";
const BROADCAST_CAPACITY: usize = 16384; // Increased for multiple symbols

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting Order Book Visualizer Backend");
    tracing::info!(
        "Tracking {} trading pairs: {:?}",
        TRADING_PAIRS.len(),
        TRADING_PAIRS
    );

    let orderbook_manager = create_shared_orderbook_manager();
    let metrics = create_shared_metrics();
    let (client_broadcast_tx, _) = broadcast::channel::<ClientMessage>(BROADCAST_CAPACITY);

    let symbols: Vec<String> = TRADING_PAIRS.iter().map(|s| s.to_string()).collect();
    let exchange_connectors = vec![
        ExchangeConnector::Binance(BinanceConn::new(symbols.clone())),
        ExchangeConnector::Bybit(BybitConn::new(symbols.clone())),
        ExchangeConnector::Coinbase(CoinbaseConn::new(symbols.clone())),
        ExchangeConnector::Kraken(KrakenConn::new(symbols.clone())),
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

    // Broadcast metrics every 3 seconds (reduced from 1s for better P99 latency)
    let _metrics_ticker = {
        let _orderbook_manager = orderbook_manager.clone();
        let metrics = metrics.clone();
        let broadcast_tx = client_broadcast_tx.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                let current_metrics = metrics.compute_metrics();
                let _ = broadcast_tx.send(ClientMessage::Metrics(current_metrics));
            }
        })
    };

    // Update system metrics every 10 seconds
    let _system_metrics_updater = {
        let metrics = metrics.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            loop {
                interval.tick().await;
                metrics.update_system_metrics();
            }
        })
    };

    // Update latency percentiles every second (in background, not blocking hot path)
    let _percentile_updater = {
        let metrics = metrics.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(900));
            loop {
                interval.tick().await;
                metrics.update_latency_percentiles();
            }
        })
    };

    let exchange_handles = exchange_manager
        .start_all(client_broadcast_tx.clone())
        .await;

    tracing::info!("Starting WebSocket server on {}", SERVER_ADDR);
    let server_result =
        server::start_server(SERVER_ADDR, orderbook_manager, metrics, client_broadcast_tx).await;

    // Keep exchange handles alive
    drop(exchange_handles);

    server_result
}
