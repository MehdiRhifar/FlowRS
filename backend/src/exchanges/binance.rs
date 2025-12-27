/// Binance Futures exchange connector

use super::{DepthSnapshot, Exchange, MarketMessage};
use crate::types::{Trade, TradeSide};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::error::Error;

#[derive(Clone)]
pub struct BinanceConnector {
    symbols: Vec<String>,
}

impl BinanceConnector {
    pub fn new(symbols: Vec<String>) -> Self {
        Self { symbols }
    }

    pub fn build_subscription_url(&self, symbols: &[&str]) -> String {
        let streams = symbols
            .iter()
            .flat_map(|s| {
                vec![
                    format!("{}@depth@100ms", s.to_lowercase()),
                    format!("{}@aggTrade", s.to_lowercase()),
                ]
            })
            .collect::<Vec<_>>()
            .join("/");

        format!("wss://fstream.binance.com/stream?streams={}", streams)
    }

    pub fn parse_message(&self, raw: &str) -> Result<Option<MarketMessage>, Box<dyn Error + Send>> {
        let is_depth = raw.as_bytes().windows(6).any(|w| w == b"@depth");

        if is_depth {
            let msg: BinanceDepthStream = serde_json::from_str(raw)
                .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;
            let symbol = msg.data.symbol.clone();

            let bids: Vec<(Decimal, Decimal)> = msg
                .data
                .bids
                .iter()
                .filter_map(|(p, q)| {
                    let price = p.parse().ok()?;
                    let qty = q.parse().ok()?;
                    Some((price, qty))
                })
                .collect();

            let asks: Vec<(Decimal, Decimal)> = msg
                .data
                .asks
                .iter()
                .filter_map(|(p, q)| {
                    let price = p.parse().ok()?;
                    let qty = q.parse().ok()?;
                    Some((price, qty))
                })
                .collect();

            Ok(Some(MarketMessage::DepthUpdate {
                exchange: Exchange::Binance,
                symbol,
                bids,
                asks,
                update_id: msg.data.final_update_id,
                is_snapshot: false, // Binance always sends deltas
            }))
        } else {
            let msg: BinanceTradeStream = serde_json::from_str(raw)
                .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;
            let trade = Trade {
                exchange: "Binance".to_string(),
                symbol: msg.data.symbol.clone(),
                price: msg
                    .data
                    .price
                    .parse()
                    .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?,
                quantity: msg
                    .data
                    .qty
                    .parse()
                    .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?,
                side: if msg.data.is_buyer_maker {
                    TradeSide::Sell
                } else {
                    TradeSide::Buy
                },
                timestamp: msg.data.event_time,
            };

            Ok(Some(MarketMessage::Trade(trade)))
        }
    }

    pub async fn fetch_snapshot(
        &self,
        symbol: &str,
        limit: usize,
    ) -> Result<Option<DepthSnapshot>, Box<dyn Error + Send>> {
        let url = format!(
            "https://fapi.binance.com/fapi/v1/depth?symbol={}&limit={}",
            symbol, limit
        );

        let response: BinanceDepthResponse = reqwest::get(&url)
            .await
            .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?
            .json()
            .await
            .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

        Ok(Some(DepthSnapshot {
            bids: response.bids,
            asks: response.asks,
            last_update_id: response.last_update_id,
        }))
    }

    pub fn supported_symbols(&self) -> Vec<String> {
        self.symbols.clone()
    }
}

// Binance-specific types
#[derive(Debug, Deserialize)]
struct BinanceDepthStream {
    #[allow(dead_code)]
    stream: String,
    data: BinanceDepthUpdate,
}

#[derive(Debug, Deserialize)]
struct BinanceDepthUpdate {
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "U")]
    _first_update_id: u64,
    #[serde(rename = "u")]
    final_update_id: u64,
    #[serde(rename = "b")]
    bids: Vec<(String, String)>,
    #[serde(rename = "a")]
    asks: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
struct BinanceTradeStream {
    #[allow(dead_code)]
    stream: String,
    data: BinanceAggTrade,
}

#[derive(Debug, Deserialize)]
struct BinanceAggTrade {
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "E")]
    event_time: i64,
    #[serde(rename = "p")]
    price: String,
    #[serde(rename = "q")]
    qty: String,
    #[serde(rename = "m")]
    is_buyer_maker: bool,
}

#[derive(Debug, Deserialize)]
struct BinanceDepthResponse {
    #[serde(rename = "lastUpdateId")]
    last_update_id: u64,
    bids: Vec<(String, String)>,
    asks: Vec<(String, String)>,
}