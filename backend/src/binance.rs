use crate::metrics::SharedMetrics;
use crate::orderbook::{OrderBook, SharedOrderBookManager};
use crate::types::{
    build_binance_rest_url, build_binance_ws_url, BinanceDepthSnapshot,
    BinanceDepthStream, BinanceTradeStream, ClientMessage, ORDERBOOK_DEPTH, TRADING_PAIRS,
};
use futures_util::{future::join_all, StreamExt};
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::broadcast;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Fetch initial order book snapshot from REST API for a specific symbol
pub async fn fetch_initial_snapshot(
    symbol: &str,
) -> Result<BinanceDepthSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let url = build_binance_rest_url(symbol);
    tracing::info!("Fetching initial order book snapshot for {}...", symbol);
    let response = reqwest::get(&url).await?;
    let snapshot: BinanceDepthSnapshot = response.json().await?;
    tracing::info!(
        "[{}] Snapshot received: {} bids, {} asks, last_update_id: {}",
        symbol,
        snapshot.bids.len(),
        snapshot.asks.len(),
        snapshot.last_update_id
    );
    Ok(snapshot)
}

/// Fetch snapshots for all symbols concurrently
pub async fn fetch_all_snapshots(
    symbols: &[&str],
) -> HashMap<String, Result<BinanceDepthSnapshot, String>> {
    let futures_vec: Vec<_> = symbols
        .iter()
        .map(|symbol| {
            let symbol = symbol.to_string();
            async move {
                let result = fetch_initial_snapshot(&symbol)
                    .await
                    .map_err(|e| e.to_string());
                (symbol, result)
            }
        })
        .collect();

    let results: Vec<(String, Result<BinanceDepthSnapshot, String>)> = join_all(futures_vec).await;
    results.into_iter().collect()
}

/// Start the Binance WebSocket connection for multiple symbols
pub async fn start_binance_connection(
    orderbook_manager: SharedOrderBookManager,
    metrics: SharedMetrics,
    tx: broadcast::Sender<ClientMessage>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    loop {
        match run_binance_connection(orderbook_manager.clone(), metrics.clone(), tx.clone()).await {
            Ok(_) => {
                tracing::warn!("Binance connection closed normally, reconnecting...");
            }
            Err(e) => {
                tracing::error!("Binance connection error: {}, reconnecting in 5s...", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }

        metrics.record_reconnect();

        // Reset all order books for fresh start
        {
            let mut manager = orderbook_manager.write().await;
            for (symbol, book) in manager.iter_mut() {
                *book = OrderBook::new(symbol);
            }
        }
    }
}

async fn run_binance_connection(
    orderbook_manager: SharedOrderBookManager,
    metrics: SharedMetrics,
    tx: broadcast::Sender<ClientMessage>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Fetch initial snapshots for all symbols concurrently
    let snapshots = fetch_all_snapshots(TRADING_PAIRS).await;

    // Initialize order books from snapshots
    {
        let mut manager = orderbook_manager.write().await;
        for (symbol, result) in snapshots {
            match result {
                Ok(snapshot) => {
                    if let Some(book) = manager.get_mut(&symbol) {
                        book.initialize_from_snapshot(
                            snapshot.bids,
                            snapshot.asks,
                            snapshot.last_update_id,
                        );
                        let msg = book.to_client_message(ORDERBOOK_DEPTH);
                        let _ = tx.send(msg);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to fetch snapshot for {}: {}", symbol, e);
                }
            }
        }

        tracing::info!(
            "Initialized {}/{} order books",
            manager.initialized_count(),
            TRADING_PAIRS.len()
        );
    }

    // Build WebSocket URL for all symbols
    let ws_url = build_binance_ws_url(TRADING_PAIRS);
    tracing::info!("Connecting to Binance WebSocket for {} symbols...", TRADING_PAIRS.len());
    tracing::debug!("WebSocket URL: {}", ws_url);

    let (ws_stream, _) = connect_async(&ws_url).await?;
    tracing::info!("Connected to Binance WebSocket");

    let (_write, mut read) = ws_stream.split();

    while let Some(message) = read.next().await {
        let start = Instant::now();

        match message {
            Ok(Message::Text(text)) => {
                metrics.record_bytes(text.len() as u64);

                let text_bytes = text.as_bytes();
                let is_depth = text_bytes.windows(6).any(|w| w == b"@depth");

                if is_depth {
                    if let Ok(msg) = serde_json::from_slice::<BinanceDepthStream>(text_bytes) {
                        let symbol = extract_symbol_from_stream(&msg.stream);
                        metrics.record_message_for_symbol(&symbol);

                        process_depth_update(
                            &orderbook_manager,
                            &metrics,
                            &tx,
                            &symbol,
                            msg.data,
                        )
                        .await;

                        metrics.record_latency_with_symbol(&symbol, start);
                    }
                } else {
                    if let Ok(msg) = serde_json::from_slice::<BinanceTradeStream>(text_bytes) {
                        let symbol = extract_symbol_from_stream(&msg.stream);
                        metrics.record_message_for_symbol(&symbol);

                        if let Some(trade) = msg.data.to_trade() {
                            metrics.record_trade_for_symbol(&symbol);
                            let _ = tx.send(ClientMessage::Trade(trade));
                        }

                        metrics.record_latency_with_symbol(&symbol, start);
                    }
                }
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

/// Extract symbol from stream name (e.g., "btcusdt@depth@100ms" -> "BTCUSDT")
fn extract_symbol_from_stream(stream: &str) -> String {
    stream
        .split('@')
        .next()
        .unwrap_or("")
        .to_uppercase()
}

async fn process_depth_update(
    orderbook_manager: &SharedOrderBookManager,
    metrics: &SharedMetrics,
    tx: &broadcast::Sender<ClientMessage>,
    symbol: &str,
    depth: crate::types::BinanceDepthUpdate,
) {
    let mut manager = orderbook_manager.write().await;

    if let Some(book) = manager.get_mut(symbol) {
        let changed = book.apply_update(
            depth.bids,
            depth.asks,
            depth.first_update_id,
            depth.final_update_id,
        );

        book.trim(ORDERBOOK_DEPTH);

        if changed {
            metrics.record_update();
            let msg = book.to_client_message(ORDERBOOK_DEPTH);
            let _ = tx.send(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::build_binance_ws_url;

    #[test]
    fn test_extract_symbol() {
        assert_eq!(extract_symbol_from_stream("btcusdt@depth@100ms"), "BTCUSDT");
        assert_eq!(extract_symbol_from_stream("ethusdt@aggTrade"), "ETHUSDT");
        assert_eq!(extract_symbol_from_stream("solusdt@depth"), "SOLUSDT");
    }

    #[test]
    fn test_build_ws_url() {
        let url = build_binance_ws_url(&["BTCUSDT", "ETHUSDT"]);
        assert!(url.contains("btcusdt@depth@100ms"));
        assert!(url.contains("btcusdt@aggTrade"));
        assert!(url.contains("ethusdt@depth@100ms"));
        assert!(url.contains("ethusdt@aggTrade"));
    }
}
