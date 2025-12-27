/// Kraken exchange connector (WebSocket v2)

use super::{DepthSnapshot, Exchange, MarketMessage};
use crate::types::{Trade, TradeSide};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::error::Error;

#[derive(Clone)]
pub struct KrakenConnector {
    symbols: Vec<String>,
}

impl KrakenConnector {
    pub fn new(symbols: Vec<String>) -> Self {
        Self { symbols }
    }

    /// Build WebSocket URL (Kraken uses base URL only)
    pub fn build_subscription_url(&self, _symbols: &[&str]) -> String {
        "wss://ws.kraken.com/v2".to_string()
    }

    /// Get subscription messages (Kraken requires post-connection subscription)
    pub fn get_subscription_messages(&self) -> Vec<String> {
        let symbols: Vec<String> = self
            .symbols
            .iter()
            .map(|s| {
                // Convert BTCUSDT -> BTC/USD format
                let base = s.trim_end_matches("USDT");
                format!("{}/USD", base)
            })
            .collect();

        // Subscribe to both book and trade channels
        let subscriptions = vec![
            KrakenSubscribe {
                method: "subscribe".to_string(),
                params: KrakenSubscribeParams {
                    channel: "book".to_string(),
                    symbol: symbols.clone(),
                    depth: Some(25),
                    snapshot: Some(true),
                },
            },
            KrakenSubscribe {
                method: "subscribe".to_string(),
                params: KrakenSubscribeParams {
                    channel: "trade".to_string(),
                    symbol: symbols,
                    depth: None,
                    snapshot: None,
                },
            },
        ];

        // Return both subscriptions as separate messages
        subscriptions
            .iter()
            .filter_map(|sub| serde_json::to_string(sub).ok())
            .collect()
    }

    pub fn parse_message(&self, raw: &str) -> Result<Option<MarketMessage>, Box<dyn Error + Send>> {
        // Check if it's a subscription confirmation, status, or heartbeat message
        if raw.contains("\"method\":\"subscribe\"")
            || raw.contains("\"channel\":\"heartbeat\"")
            || raw.contains("\"channel\":\"status\"") {
            tracing::debug!("[Kraken] Ignoring status/heartbeat message");
            return Ok(None);
        }

        // Parse channel type first
        let channel_check: serde_json::Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => {
                let preview = if raw.len() > 200 { &raw[..200] } else { raw };
                tracing::warn!("[Kraken] Failed to parse message: {} - Preview: {}", e, preview);
                return Ok(None);
            }
        };

        let channel = channel_check["channel"].as_str().unwrap_or("");

        match channel {
            "book" => self.parse_book_message(raw),
            "trade" => self.parse_trade_message(raw),
            _ => {
                tracing::debug!("[Kraken] Ignoring channel: {}", channel);
                Ok(None)
            }
        }
    }

    fn parse_book_message(&self, raw: &str) -> Result<Option<MarketMessage>, Box<dyn Error + Send>> {
        let msg: KrakenBookMessage = serde_json::from_str(raw)
            .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

        for data in msg.data {
            // Convert BTC/USD -> BTCUSDT
            let symbol = data.symbol.replace("/USD", "USDT");
            let is_snapshot = msg.type_ == "snapshot";

            let bids: Vec<(Decimal, Decimal)> = data
                .bids
                .iter()
                .filter_map(|b| {
                    let price = Decimal::from_f64_retain(b.price)?;
                    let qty = Decimal::from_f64_retain(b.qty)?;
                    Some((price, qty))
                })
                .collect();

            let asks: Vec<(Decimal, Decimal)> = data
                .asks
                .iter()
                .filter_map(|a| {
                    let price = Decimal::from_f64_retain(a.price)?;
                    let qty = Decimal::from_f64_retain(a.qty)?;
                    Some((price, qty))
                })
                .collect();

            // Skip empty updates
            if !is_snapshot && bids.is_empty() && asks.is_empty() {
                continue;
            }

            return Ok(Some(MarketMessage::DepthUpdate {
                exchange: Exchange::Kraken,
                symbol,
                bids,
                asks,
                update_id: data.checksum.unwrap_or(0) as u64,
                is_snapshot,
            }));
        }

        Ok(None)
    }

    fn parse_trade_message(&self, raw: &str) -> Result<Option<MarketMessage>, Box<dyn Error + Send>> {
        let msg: KrakenTradeMessage = serde_json::from_str(raw)
            .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;
        for data in msg.data {
            // Convert BTC/USD -> BTCUSDT
            let symbol = data.symbol.replace("/USD", "USDT");

            // Kraken envoie des f64, on les convertit en Decimal
            let price = Decimal::from_f64_retain(data.price)
                .ok_or_else(|| Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid price")) as Box<dyn Error + Send>)?;
            let quantity = Decimal::from_f64_retain(data.qty)
                .ok_or_else(|| Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid quantity")) as Box<dyn Error + Send>)?;

            let side = match data.side.as_str() {
                "buy" => TradeSide::Buy,
                "sell" => TradeSide::Sell,
                _ => continue,
            };

            // Kraken timestamp est en format ISO 8601, parser vers milliseconds
            let timestamp = chrono::DateTime::parse_from_rfc3339(&data.timestamp)
                .map(|dt| dt.timestamp_millis())
                .unwrap_or(0);

            let trade = Trade {
                exchange: "Kraken".to_string(),
                symbol,
                price,
                quantity,
                side,
                timestamp,
            };

            return Ok(Some(MarketMessage::Trade(trade)));
        }

        Ok(None)
    }

    /// Kraken sends initial snapshot via WebSocket, so REST fetch not needed
    pub async fn fetch_snapshot(
        &self,
        _symbol: &str,
        _limit: usize,
    ) -> Result<Option<DepthSnapshot>, Box<dyn Error + Send>> {
        // Kraken will send snapshot via WebSocket after subscription - no need for REST fetch
        Ok(None)
    }

    pub fn supported_symbols(&self) -> Vec<String> {
        self.symbols.clone()
    }
}

// Kraken-specific types

#[derive(Debug, Deserialize)]
struct KrakenBookMessage {
    #[allow(dead_code)]
    _channel: String,
    #[serde(rename = "type")]
    type_: String,
    data: Vec<KrakenBookData>,
}

#[derive(Debug, Deserialize)]
struct KrakenBookData {
    symbol: String,
    bids: Vec<KrakenPriceLevel>,
    asks: Vec<KrakenPriceLevel>,
    checksum: Option<i64>,
    #[allow(dead_code)]
    timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
struct KrakenPriceLevel {
    price: f64,  // Kraken sends numbers, not strings
    qty: f64,
}

#[derive(Debug, serde::Serialize)]
struct KrakenSubscribe {
    method: String,
    params: KrakenSubscribeParams,
}

#[derive(Debug, serde::Serialize)]
struct KrakenSubscribeParams {
    channel: String,
    symbol: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    depth: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct KrakenTradeMessage {
    #[allow(dead_code)]
    channel: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    type_: String,
    data: Vec<KrakenTradeData>,
}

#[derive(Debug, Deserialize)]
struct KrakenTradeData {
    symbol: String,
    price: f64,        // Kraken envoie un nombre, pas une string
    qty: f64,          // Kraken envoie un nombre, pas une string
    side: String,
    timestamp: String, // ISO 8601 format
    #[allow(dead_code)]
    ord_type: Option<String>,
    #[allow(dead_code)]
    trade_id: Option<u64>,
}