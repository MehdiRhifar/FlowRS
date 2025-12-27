/// Coinbase Advanced Trade exchange connector

use super::{DepthSnapshot, Exchange, MarketMessage};
use crate::types::{Trade, TradeSide};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::error::Error;

#[derive(Clone)]
pub struct CoinbaseConnector {
    symbols: Vec<String>,
}

impl CoinbaseConnector {
    pub fn new(symbols: Vec<String>) -> Self {
        Self { symbols }
    }

    /// Build WebSocket URL (Coinbase uses base URL only)
    pub fn build_subscription_url(&self, _symbols: &[&str]) -> String {
        "wss://advanced-trade-ws.coinbase.com".to_string()
    }

    /// Get subscription messages (Coinbase requires post-connection subscription)
    pub fn get_subscription_messages(&self) -> Vec<String> {
        let product_ids: Vec<String> = self
            .symbols
            .iter()
            .map(|s| {
                // Convert BTCUSDT -> BTC-USD format
                let base = s.trim_end_matches("USDT");
                format!("{}-USD", base)
            })
            .collect();

        // Subscribe to both level2 (orderbook) and market_trades channels
        let subscriptions = vec![
            CoinbaseSubscribe {
                type_: "subscribe".to_string(),
                product_ids: product_ids.clone(),
                channel: "level2".to_string(),
            },
            CoinbaseSubscribe {
                type_: "subscribe".to_string(),
                product_ids,
                channel: "market_trades".to_string(),
            },
        ];

        // Return both subscriptions as separate messages
        subscriptions
            .iter()
            .filter_map(|sub| serde_json::to_string(sub).ok())
            .collect()
    }

    pub fn parse_message(&self, raw: &str) -> Result<Option<MarketMessage>, Box<dyn Error + Send>> {
        // Check if it's a subscription confirmation or other status message
        if raw.contains("\"channel\":\"subscriptions\"")
            || raw.contains("\"channel\":\"heartbeats\"")
            || raw.contains("\"subscriptions\":{") {
            tracing::debug!("[Coinbase] Ignoring subscription/heartbeat");
            return Ok(None);
        }

        // Parse channel type first
        let channel_check: serde_json::Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => {
                let preview = if raw.len() > 200 { &raw[..200] } else { raw };
                tracing::warn!("[Coinbase] Failed to parse message: {} - Preview: {}", e, preview);
                return Ok(None);
            }
        };

        let channel = channel_check["channel"].as_str().unwrap_or("");

        match channel {
            "l2_data" => self.parse_level2_message(raw),
            "market_trades" => self.parse_trade_message(raw),
            _ => {
                tracing::debug!("[Coinbase] Ignoring channel: {}", channel);
                Ok(None)
            }
        }
    }

    fn parse_level2_message(&self, raw: &str) -> Result<Option<MarketMessage>, Box<dyn Error + Send>> {
        let msg: CoinbaseLevel2Message = serde_json::from_str(raw)
            .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

        for event in msg.events {
            // Convert BTC-USD -> BTCUSDT
            let symbol = event.product_id.replace("-USD", "USDT");

            let mut bids = Vec::new();
            let mut asks = Vec::new();

            for update in event.updates {
                let price: Decimal = update
                    .price_level
                    .parse()
                    .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;
                let qty: Decimal = update
                    .new_quantity
                    .parse()
                    .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

                match update.side.as_str() {
                    "bid" => bids.push((price, qty)),
                    "offer" => asks.push((price, qty)),
                    _ => {}
                }
            }

            let is_snapshot = event.type_ == "snapshot";

            return Ok(Some(MarketMessage::DepthUpdate {
                exchange: Exchange::Coinbase,
                symbol,
                bids,
                asks,
                update_id: msg.sequence_num,
                is_snapshot,
            }));
        }

        Ok(None)
    }

    fn parse_trade_message(&self, raw: &str) -> Result<Option<MarketMessage>, Box<dyn Error + Send>> {
        let msg: CoinbaseTradeMessage = serde_json::from_str(raw)
            .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

        for event in msg.events {
            for trade_data in &event.trades {
                // Convert BTC-USD -> BTCUSDT
                let symbol = trade_data.product_id.replace("-USD", "USDT");

                let price: Decimal = trade_data.price.parse()
                    .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;
                let quantity: Decimal = trade_data.size.parse()
                    .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

                let side = match trade_data.side.as_str() {
                    "BUY" => TradeSide::Buy,
                    "SELL" => TradeSide::Sell,
                    _ => continue,
                };

                // Parse timestamp from ISO format to milliseconds
                let timestamp = chrono::DateTime::parse_from_rfc3339(&trade_data.time)
                    .map(|dt| dt.timestamp_millis())
                    .unwrap_or(0);

                let trade = Trade {
                    exchange: "Coinbase".to_string(),
                    symbol,
                    price,
                    quantity,
                    side,
                    timestamp,
                };

                return Ok(Some(MarketMessage::Trade(trade)));
            }
        }

        Ok(None)
    }

    /// Coinbase sends initial snapshot via WebSocket, so REST fetch not needed
    pub async fn fetch_snapshot(
        &self,
        _symbol: &str,
        _limit: usize,
    ) -> Result<Option<DepthSnapshot>, Box<dyn Error + Send>> {
        // Coinbase will send snapshot via WebSocket after subscription - no need for REST fetch
        Ok(None)
    }

    pub fn supported_symbols(&self) -> Vec<String> {
        self.symbols.clone()
    }
}

// Coinbase-specific types

#[derive(Debug, Deserialize)]
struct CoinbaseLevel2Message {
    #[allow(dead_code)]
    _channel: String,
    #[allow(dead_code)]
    client_id: String,
    #[allow(dead_code)]
    timestamp: String,
    sequence_num: u64,
    events: Vec<CoinbaseLevel2Event>,
}

#[derive(Debug, Deserialize)]
struct CoinbaseLevel2Event {
    #[serde(rename = "type")]
    type_: String,
    product_id: String,
    updates: Vec<CoinbaseLevel2Update>,
}

#[derive(Debug, Deserialize)]
struct CoinbaseLevel2Update {
    side: String,
    #[allow(dead_code)]
    event_time: String,
    price_level: String,
    new_quantity: String,
}

#[derive(Debug, serde::Serialize)]
struct CoinbaseSubscribe {
    #[serde(rename = "type")]
    type_: String,
    product_ids: Vec<String>,
    channel: String,
}

#[derive(Debug, Deserialize)]
struct CoinbaseTradeMessage {
    #[allow(dead_code)]
    channel: String,
    #[allow(dead_code)]
    client_id: String,
    #[allow(dead_code)]
    timestamp: String,
    #[allow(dead_code)]
    sequence_num: u64,
    events: Vec<CoinbaseTradeEvent>,
}

#[derive(Debug, Deserialize)]
struct CoinbaseTradeEvent {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    type_: String,
    trades: Vec<CoinbaseTradeData>,
}

#[derive(Debug, Deserialize)]
struct CoinbaseTradeData {
    #[serde(rename = "trade_id")]
    #[allow(dead_code)]
    trade_id: String,
    product_id: String,
    price: String,
    size: String,
    side: String,
    time: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coinbase_url_building() {
        let connector = CoinbaseConnector::new(vec!["BTCUSDT".to_string()]);
        let url = connector.build_subscription_url(&["BTCUSDT"]);
        assert_eq!(url, "wss://advanced-trade-ws.coinbase.com");
    }
}
