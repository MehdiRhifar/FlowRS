use rust_decimal::Decimal;
use serde::{Deserialize, Serialize, Serializer};

// Scale factors for u64 ↔ Decimal conversion (must match orderbook.rs)
const PRICE_FACTOR: u64 = 100_000_000; // 1e8
const QTY_FACTOR: u64 = 100_000_000; // 1e8

// Custom serializers to convert u64 → Decimal for JSON output
fn serialize_price<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let decimal = Decimal::from(*value) / Decimal::from(PRICE_FACTOR);
    Serialize::serialize(&decimal, serializer)
}

fn serialize_quantity<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let decimal = Decimal::from(*value) / Decimal::from(QTY_FACTOR);
    Serialize::serialize(&decimal, serializer)
}

/// Number of price levels to store in memory (auto-trimmed after each update)
pub const ORDERBOOK_DEPTH: usize = 25;

/// Number of price levels to send to clients (optimisation)
pub const ORDERBOOK_DISPLAY_DEPTH: usize = 3;

/// Trading pairs supported
pub const TRADING_PAIRS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "AVAXUSDT", "DOTUSDT",
    "LINKUSDT",
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
    #[serde(serialize_with = "serialize_price")]
    pub price: u64, // Scaled by PRICE_FACTOR (1e8), converted to Decimal on serialization
    #[serde(serialize_with = "serialize_quantity")]
    pub quantity: u64, // Scaled by QTY_FACTOR (1e8), converted to Decimal on serialization
    pub side: TradeSide,
    pub timestamp: i64,
}

/// Global performance metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Metrics {
    // Per-second rates
    pub messages_per_second: u64,
    pub bytes_per_second: u64,

    // Latency stats (in microseconds for precision)
    pub latency_avg_us: f64,
    pub latency_p50_us: u64,
    pub latency_p95_us: u64,
    pub latency_p99_us: u64,

    // Totals
    pub total_messages: u64,

    // System stats
    pub uptime_seconds: u64,
    pub memory_used_mb: f64,
    pub memory_rss_mb: f64,
    pub cpu_usage_percent: f64,

    // Connection stats
    pub active_connections: u32,
    pub websocket_reconnects: u64,

    // Throughput
    pub bytes_received: u64,
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
    },
    Trade(Trade),
    Metrics(Metrics),
    SymbolList(Vec<String>),
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
