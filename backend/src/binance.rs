use crate::metrics::SharedMetrics;
use crate::orderbook::SharedOrderBook;
use crate::types::{
    BinanceDepthSnapshot, BinanceDepthUpdate, BinanceStreamWrapper, BinanceTrade, ClientMessage,
};
use futures_util::StreamExt;
use std::time::Instant;
use tokio::sync::broadcast;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const BINANCE_WS_URL: &str =
    "wss://stream.binance.com:9443/stream?streams=btcusdt@depth@100ms/btcusdt@trade";
const BINANCE_REST_URL: &str = "https://api.binance.com/api/v3/depth?symbol=BTCUSDT&limit=20";

/// Fetch the initial order book snapshot from REST API
pub async fn fetch_initial_snapshot() -> Result<BinanceDepthSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    tracing::info!("Fetching initial order book snapshot...");
    let response = reqwest::get(BINANCE_REST_URL).await?;
    let snapshot: BinanceDepthSnapshot = response.json().await?;
    tracing::info!(
        "Snapshot received: {} bids, {} asks, last_update_id: {}",
        snapshot.bids.len(),
        snapshot.asks.len(),
        snapshot.last_update_id
    );
    Ok(snapshot)
}

/// Start the Binance WebSocket connection and process messages
pub async fn start_binance_connection(
    orderbook: SharedOrderBook,
    metrics: SharedMetrics,
    tx: broadcast::Sender<ClientMessage>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    loop {
        match run_binance_connection(orderbook.clone(), metrics.clone(), tx.clone()).await {
            Ok(_) => {
                tracing::warn!("Binance connection closed normally, reconnecting...");
            }
            Err(e) => {
                tracing::error!("Binance connection error: {}, reconnecting in 5s...", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }

        // Reset order book for fresh start
        {
            let mut book = orderbook.write().await;
            *book = crate::orderbook::OrderBook::new();
        }
    }
}

async fn run_binance_connection(
    orderbook: SharedOrderBook,
    metrics: SharedMetrics,
    tx: broadcast::Sender<ClientMessage>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Fetch initial snapshot
    let snapshot = fetch_initial_snapshot().await?;
    let snapshot_update_id = snapshot.last_update_id;

    // Initialize order book
    {
        let mut book = orderbook.write().await;
        book.initialize_from_snapshot(snapshot.bids, snapshot.asks, snapshot_update_id);

        // Send initial book state to clients
        let msg = book.to_client_message(20);
        let _ = tx.send(msg);
    }

    // Buffer for updates received before snapshot was processed
    let mut pending_updates: Vec<BinanceDepthUpdate> = Vec::new();

    // Connect to WebSocket
    tracing::info!("Connecting to Binance WebSocket...");
    let (ws_stream, _) = connect_async(BINANCE_WS_URL).await?;
    tracing::info!("Connected to Binance WebSocket");

    let (mut _write, mut read) = ws_stream.split();

    while let Some(message) = read.next().await {
        let start = Instant::now();

        match message {
            Ok(Message::Text(text)) => {
                metrics.record_message();

                // Parse the stream wrapper
                if let Ok(wrapper) = serde_json::from_str::<BinanceStreamWrapper>(&text) {
                    if wrapper.stream.contains("depth") {
                        // Depth update
                        if let Ok(depth) =
                            serde_json::from_value::<BinanceDepthUpdate>(wrapper.data)
                        {
                            process_depth_update(
                                &orderbook,
                                &metrics,
                                &tx,
                                depth,
                                snapshot_update_id,
                                &mut pending_updates,
                            )
                            .await;
                        }
                    } else if wrapper.stream.contains("trade") {
                        // Trade event
                        if let Ok(trade) = serde_json::from_value::<BinanceTrade>(wrapper.data) {
                            if let Some(trade) = trade.to_trade() {
                                let _ = tx.send(ClientMessage::Trade(trade));
                            }
                        }
                    }
                }

                metrics.record_latency(start);
            }
            Ok(Message::Ping(_)) => {
                tracing::debug!("Received ping, pong handled automatically");
            }
            Ok(Message::Close(_)) => {
                tracing::warn!("WebSocket connection closed by server");
                break;
            }
            Err(e) => {
                tracing::error!("WebSocket error: {}", e);
                return Err(e.into());
            }
            _ => {}
        }
    }

    Ok(())
}

async fn process_depth_update(
    orderbook: &SharedOrderBook,
    metrics: &SharedMetrics,
    tx: &broadcast::Sender<ClientMessage>,
    depth: BinanceDepthUpdate,
    _snapshot_update_id: u64,
    _pending_updates: &mut Vec<BinanceDepthUpdate>,
) {
    let mut book = orderbook.write().await;

    // Apply the update
    let changed = book.apply_update(
        depth.bids,
        depth.asks,
        depth.first_update_id,
        depth.final_update_id,
    );

    // Trim the book to prevent accumulation of stale levels
    book.trim(50);

    if changed {
        metrics.record_update();
        let msg = book.to_client_message(20);
        let _ = tx.send(msg);
    }
}
