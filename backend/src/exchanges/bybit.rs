/// Bybit exchange connector

use super::{DepthSnapshot, Exchange, MarketMessage};
use crate::types::{Trade, TradeSide};
use rust_decimal::Decimal;
use std::error::Error;

#[derive(Clone)]
pub struct BybitConnector {
    symbols: Vec<String>,
}

impl BybitConnector {
    pub fn new(symbols: Vec<String>) -> Self {
        Self { symbols }
    }

    pub fn build_subscription_url(&self, _symbols: &[&str]) -> String {
        // Bybit uses a different subscription model (subscribe after connection)
        "wss://stream.bybit.com/v5/public/linear".to_string()
    }


    /// Build subscription messages for Bybit WebSocket
    pub fn get_subscription_messages(&self, symbols: &[&str]) -> Vec<String> {
        let args: Vec<String> = symbols
            .iter()
            .flat_map(|s| {
                vec![
                    format!("orderbook.50.{}", s),
                    format!("publicTrade.{}", s),
                ]
            })
            .collect();

        let subscription = serde_json::json!({
            "op": "subscribe",
            "args": args
        });

        vec![subscription.to_string()]
    }

    pub fn parse_message(&self, raw: &str) -> Result<Option<MarketMessage>, Box<dyn Error + Send>> {
        let msg: serde_json::Value = serde_json::from_str(raw)
            .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

        // Bybit format: {"topic": "orderbook.25.BTCUSDT", "type": "snapshot|delta", "data": {...}}
        if let Some(topic) = msg["topic"].as_str() {
            // Handle orderbook updates
            if topic.starts_with("orderbook") {
                let parts: Vec<&str> = topic.split('.').collect();
                if parts.len() >= 3 {
                    let symbol = parts[2].to_string();
                    let msg_type = msg["type"].as_str().unwrap_or("delta");

                    if msg_type == "snapshot" {
                        tracing::debug!("[Bybit] Received snapshot for {}", symbol);
                    }

                    let bids: Vec<(Decimal, Decimal)> = msg["data"]["b"]
                        .as_array()
                        .unwrap_or(&vec![])
                        .iter()
                        .filter_map(|item| {
                            let price = item[0].as_str()?.parse().ok()?;
                            let qty = item[1].as_str()?.parse().ok()?;
                            Some((price, qty))
                        })
                        .collect();

                    let asks: Vec<(Decimal, Decimal)> = msg["data"]["a"]
                        .as_array()
                        .unwrap_or(&vec![])
                        .iter()
                        .filter_map(|item| {
                            let price = item[0].as_str()?.parse().ok()?;
                            let qty = item[1].as_str()?.parse().ok()?;
                            Some((price, qty))
                        })
                        .collect();

                    let update_id = msg["data"]["u"].as_u64().unwrap_or(0);
                    let is_snapshot = msg_type == "snapshot";

                    return Ok(Some(MarketMessage::DepthUpdate {
                        exchange: Exchange::Bybit,
                        symbol,
                        bids,
                        asks,
                        update_id,
                        is_snapshot,
                    }));
                }
            }
            // Handle trade updates
            else if topic.starts_with("publicTrade") {
                let parts: Vec<&str> = topic.split('.').collect();
                if parts.len() >= 2 {
                    let symbol = parts[1].to_string();

                    if let Some(trades_array) = msg["data"].as_array() {
                        for trade_data in trades_array {
                            if let (Some(price_str), Some(qty_str), Some(side_str)) = (
                                trade_data["p"].as_str(),
                                trade_data["v"].as_str(),
                                trade_data["S"].as_str(),
                            ) {
                                let price: Decimal = price_str.parse()
                                    .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

                                let quantity: Decimal = qty_str.parse()
                                    .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

                                let side = match side_str {
                                    "Buy" => TradeSide::Buy,
                                    "Sell" => TradeSide::Sell,
                                    _ => continue,
                                };

                                let timestamp = trade_data["T"].as_i64().unwrap_or(0);

                                let trade = Trade {
                                    exchange: "Bybit".to_string(),
                                    symbol: symbol.clone(),
                                    price,
                                    quantity,
                                    side,
                                    timestamp,
                                };

                                return Ok(Some(MarketMessage::Trade(trade)));
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    /// Bybit sends initial snapshot via WebSocket, so REST fetch not needed
    pub async fn fetch_snapshot(
        &self,
        _symbol: &str,
        _limit: usize,
    ) -> Result<Option<DepthSnapshot>, Box<dyn Error + Send>> {
        // Bybit will send snapshot via WebSocket after subscription - no need for REST fetch
        Ok(None)
    }

    pub fn supported_symbols(&self) -> Vec<String> {
        self.symbols.clone()
    }
}
