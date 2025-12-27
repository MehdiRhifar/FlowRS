use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Number of price levels to store in memory
pub const ORDERBOOK_DEPTH: usize = 50;

/// Number of price levels to send to clients (optimisation)
pub const ORDERBOOK_DISPLAY_DEPTH: usize = 5;

/// Trading pairs supported
pub const TRADING_PAIRS: &[&str] = &[
    "BTCUSDT",
    "ETHUSDT",
    "BNBUSDT",
    "SOLUSDT",
    "XRPUSDT",
    "DOGEUSDT",
    "ADAUSDT",
    "AVAXUSDT",
    "DOTUSDT",
];

/// Price level in the order book
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

/// Trade side
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TradeSide {
    Buy,
    Sell,
}

/// A single trade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub exchange: String,
    pub symbol: String,
    pub price: Decimal,
    pub quantity: Decimal,
    pub side: TradeSide,
    pub timestamp: i64,
}

/// Per-symbol metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SymbolMetrics {
    pub messages_per_second: u64,
    pub trades_per_second: u64,
    pub latency_avg_us: f64,
    pub spread_bps: Option<f64>,
}

/// Global performance metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Metrics {
    // Per-second rates
    pub messages_per_second: u64,
    pub updates_per_second: u64,
    pub trades_per_second: u64,
    
    // Latency stats (in microseconds for precision)
    pub latency_avg_us: f64,
    pub latency_min_us: u64,
    pub latency_max_us: u64,
    pub latency_p50_us: u64,
    pub latency_p95_us: u64,
    pub latency_p99_us: u64,
    
    // Totals
    pub total_messages: u64,
    pub total_updates: u64,
    pub total_trades: u64,
    
    // System stats
    pub uptime_seconds: u64,
    pub memory_used_mb: f64,
    pub memory_rss_mb: f64,
    pub cpu_usage_percent: f64,
    
    // Connection stats
    pub active_symbols: u32,
    pub active_connections: u32,
    pub websocket_reconnects: u64,
    
    // Throughput
    pub bytes_received: u64,
    pub bytes_per_second: u64,
    
    // Per-symbol breakdown
    pub symbols: HashMap<String, SymbolMetrics>,
}

/// Messages sent to frontend clients
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum ClientMessage {
    BookUpdate {
        exchange: String,
        symbol: String,
        bids: Vec<PriceLevel>,
        asks: Vec<PriceLevel>,
        spread: Decimal,
        spread_percent: Decimal,
        bid_depth: Decimal,
        ask_depth: Decimal,
    },
    Trade(Trade),
    Metrics(Metrics),
    SymbolList(Vec<String>),
}

/// Binance depth snapshot response
/// Used by benchmarks and exchanges/binance.rs
#[derive(Debug, Deserialize)]
pub struct BinanceDepthSnapshot {
    #[serde(rename = "lastUpdateId")]
    pub last_update_id: u64,
    pub bids: Vec<(String, String)>,
    pub asks: Vec<(String, String)>,
}

/// Binance stream message for depth updates
/// Used by benchmarks and exchanges/binance.rs
#[derive(Debug, Deserialize)]
pub struct BinanceDepthStream {
    pub stream: String,
    pub data: BinanceDepthUpdate,
}

/// Binance stream message for trades
/// Used by benchmarks and exchanges/binance.rs
#[derive(Debug, Deserialize)]
pub struct BinanceTradeStream {
    pub stream: String,
    pub data: BinanceAggTrade,
}

/// Binance depth update event
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct BinanceDepthUpdate {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "U")]
    pub first_update_id: u64,
    #[serde(rename = "u")]
    pub final_update_id: u64,
    #[serde(rename = "b")]
    pub bids: Vec<(String, String)>,
    #[serde(rename = "a")]
    pub asks: Vec<(String, String)>,
}

/// Binance aggregate trade event (Futures)
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct BinanceAggTrade {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "a")]
    pub agg_trade_id: u64,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "q")]
    pub quantity: String,
    #[serde(rename = "f")]
    pub first_trade_id: u64,
    #[serde(rename = "l")]
    pub last_trade_id: u64,
    #[serde(rename = "T")]
    pub trade_time: i64,
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

impl BinanceAggTrade {
    pub fn to_trade(&self) -> Option<Trade> {
        let price = self.price.parse().ok()?;
        let quantity = self.quantity.parse().ok()?;
        let side = if self.is_buyer_maker {
            TradeSide::Sell
        } else {
            TradeSide::Buy
        };
        Some(Trade {
            exchange: "Binance".to_string(),
            symbol: self.symbol.clone(),
            price,
            quantity,
            side,
            timestamp: self.trade_time,
        })
    }
}

/// Helper to build Binance stream URLs for multiple symbols
pub fn build_binance_ws_url(symbols: &[&str]) -> String {
    let streams: Vec<String> = symbols
        .iter()
        .flat_map(|s| {
            let lower = s.to_lowercase();
            vec![
                format!("{}@depth@100ms", lower),
                format!("{}@aggTrade", lower),
            ]
        })
        .collect();
    format!("wss://fstream.binance.com/stream?streams={}", streams.join("/"))
}

/// Helper to build Binance REST URL for depth snapshot
pub fn build_binance_rest_url(symbol: &str) -> String {
    format!("https://fapi.binance.com/fapi/v1/depth?symbol={}&limit=10", symbol.to_uppercase())
}
