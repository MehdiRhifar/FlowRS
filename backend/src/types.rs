use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

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
    pub price: Decimal,
    pub quantity: Decimal,
    pub side: TradeSide,
    pub timestamp: i64,
}

/// Performance metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Metrics {
    pub messages_per_second: u64,
    pub latency_avg_ms: f64,
    pub updates_per_second: u64,
    pub uptime_seconds: u64,
    pub memory_used_mb: f64,
}

/// Messages sent to frontend clients
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum ClientMessage {
    BookUpdate {
        bids: Vec<PriceLevel>,
        asks: Vec<PriceLevel>,
        spread: Decimal,
        spread_percent: Decimal,
    },
    Trade(Trade),
    Metrics(Metrics),
}

// ============ Binance API Types ============

/// Binance depth snapshot response
#[derive(Debug, Deserialize)]
pub struct BinanceDepthSnapshot {
    #[serde(rename = "lastUpdateId")]
    pub last_update_id: u64,
    pub bids: Vec<(String, String)>,
    pub asks: Vec<(String, String)>,
}

/// Binance combined stream wrapper
#[derive(Debug, Deserialize)]
pub struct BinanceStreamWrapper {
    pub stream: String,
    pub data: serde_json::Value,
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

/// Binance trade event
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct BinanceTrade {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "t")]
    pub trade_id: u64,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "q")]
    pub quantity: String,
    #[serde(rename = "T")]
    pub trade_time: i64,
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

impl BinanceTrade {
    pub fn to_trade(&self) -> Option<Trade> {
        let price = self.price.parse().ok()?;
        let quantity = self.quantity.parse().ok()?;
        let side = if self.is_buyer_maker {
            TradeSide::Sell
        } else {
            TradeSide::Buy
        };
        Some(Trade {
            price,
            quantity,
            side,
            timestamp: self.trade_time,
        })
    }
}
