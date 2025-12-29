use super::utils::fast_parse_u64_inner;
use super::{DepthSnapshot, Exchange, MarketMessage};
use crate::types::{Trade, TradeSide};
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

    pub fn build_subscription_url(&self, _symbols: &[&str]) -> String {
        "wss://advanced-trade-ws.coinbase.com".to_string()
    }

    pub fn get_subscription_messages(&self) -> Vec<String> {
        // Optimisation: vec! macro vs push manuel n'a pas bcp d'impact ici car c'est fait 1 fois
        // Mais on prépare proprement les product_ids
        let product_ids: Vec<String> = self
            .symbols
            .iter()
            .map(|s| {
                let base = s.trim_end_matches("USDT");
                format!("{}-USD", base)
            })
            .collect();

        // On clone product_ids car utilisé 2 fois, c'est inévitable mais négligeable (init)
        let sub_l2 = CoinbaseSubscribe {
            type_: "subscribe",
            product_ids: product_ids.clone(),
            channel: "level2",
        };

        let sub_trades = CoinbaseSubscribe {
            type_: "subscribe",
            product_ids,
            channel: "market_trades",
        };

        vec![
            serde_json::to_string(&sub_l2).unwrap_or_default(),
            serde_json::to_string(&sub_trades).unwrap_or_default(),
        ]
    }

    /// Cœur du réacteur : Parsing Zero-Copy
    pub fn parse_message(&self, raw: &str) -> Result<Option<MarketMessage>, Box<dyn Error + Send>> {
        // 1. Filtrage ultra-rapide (SIMD friendly) des messages de contrôle
        // On check les chaînes brutes pour éviter tout parsing JSON si inutile
        if raw.contains(r#""channel":"subscriptions""#)
            || raw.contains(r#""channel":"heartbeats""#)
            || raw.contains(r#""subscriptions":{"#)
        {
            return Ok(None);
        }

        // 2. Parsing partiel "Zero-Copy" pour router le message
        // On ne décode que le strict nécessaire pour savoir quel parser lancer
        #[derive(Deserialize)]
        struct ChannelHeader<'a> {
            channel: &'a str,
        }

        let header: ChannelHeader = match serde_json::from_str(raw) {
            Ok(h) => h,
            Err(_) => return Ok(None), // Ignorer les erreurs de parsing (bruit)
        };

        match header.channel {
            "l2_data" => self.parse_level2_message(raw),
            "market_trades" => self.parse_trade_message(raw),
            _ => Ok(None),
        }
    }

    fn parse_level2_message(
        &self,
        raw: &str,
    ) -> Result<Option<MarketMessage>, Box<dyn Error + Send>> {
        // Zero-Copy deserialization: les champs &'a str pointent dans 'raw'
        let msg: CoinbaseLevel2Message =
            serde_json::from_str(raw).map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

        // Coinbase envoie souvent 1 seul event, on prend le premier
        if let Some(event) = msg.events.first() {
            // Transformation du symbole : allocation obligatoire ici pour le String final
            // Optimisation possible : utiliser un cache de symboles si la liste est fixe
            let symbol = event.product_id.replace("-USD", "USDT");

            // Collect avec filter_map : allocation exacte, pas de boucle + push
            let bids: Vec<(u64, u64)> = event
                .updates
                .iter()
                .filter(|u| u.side == "bid")
                .filter_map(|update| {
                    let price = fast_parse_u64_inner(update.price_level)?;
                    let qty = fast_parse_u64_inner(update.new_quantity)?;
                    Some((price, qty))
                })
                .collect();

            let asks: Vec<(u64, u64)> = event
                .updates
                .iter()
                .filter(|u| u.side == "offer")
                .filter_map(|update| {
                    let price = fast_parse_u64_inner(update.price_level)?;
                    let qty = fast_parse_u64_inner(update.new_quantity)?;
                    Some((price, qty))
                })
                .collect();

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

    fn parse_trade_message(
        &self,
        raw: &str,
    ) -> Result<Option<MarketMessage>, Box<dyn Error + Send>> {
        let msg: CoinbaseTradeMessage =
            serde_json::from_str(raw).map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

        if let Some(event) = msg.events.first() {
            // Pour l'instant on prend le premier trade du batch
            // TODO: Adapter MarketMessage pour accepter Vec<Trade> pour plus d'efficacité
            if let Some(trade_data) = event.trades.first() {
                let symbol = trade_data.product_id.replace("-USD", "USDT");

                let price = match fast_parse_u64_inner(trade_data.price) {
                    Some(p) => p,
                    None => return Ok(None),
                };
                let quantity = match fast_parse_u64_inner(trade_data.size) {
                    Some(q) => q,
                    None => return Ok(None),
                };

                let side = match trade_data.side {
                    "BUY" => TradeSide::Buy,
                    "SELL" => TradeSide::Sell,
                    _ => return Ok(None),
                };

                // Parsing de date : c'est souvent le goulot d'étranglement restant
                // chrono est correct, mais pour de l'ultra-perf, on parserait manuellement le timestamp
                let timestamp = chrono::DateTime::parse_from_rfc3339(trade_data.time)
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

    pub async fn fetch_snapshot(
        &self,
        _symbol: &str,
        _limit: usize,
    ) -> Result<Option<DepthSnapshot>, Box<dyn Error + Send>> {
        Ok(None)
    }

    pub fn supported_symbols(&self) -> Vec<String> {
        self.symbols.clone()
    }
}

// --- OPTIMIZED DTOs (Data Transfer Objects) ---
// Utilisation stricte de références &'a str pour le Zero-Copy

#[derive(Debug, Deserialize)]
struct CoinbaseLevel2Message<'a> {
    sequence_num: u64,
    #[serde(borrow)]
    events: Vec<CoinbaseLevel2Event<'a>>,
}

#[derive(Debug, Deserialize)]
struct CoinbaseLevel2Event<'a> {
    #[serde(rename = "type")]
    type_: &'a str,
    product_id: &'a str,
    #[serde(borrow)]
    updates: Vec<CoinbaseLevel2Update<'a>>,
}

#[derive(Debug, Deserialize)]
struct CoinbaseLevel2Update<'a> {
    side: &'a str,
    price_level: &'a str,
    new_quantity: &'a str,
}

#[derive(Debug, serde::Serialize)]
struct CoinbaseSubscribe<'a> {
    #[serde(rename = "type")]
    type_: &'a str, // Optimisation: 'subscribe' est statique, pas besoin de String
    product_ids: Vec<String>,
    channel: &'a str,
}

#[derive(Debug, Deserialize)]
struct CoinbaseTradeMessage<'a> {
    #[serde(borrow)]
    events: Vec<CoinbaseTradeEvent<'a>>,
}

#[derive(Debug, Deserialize)]
struct CoinbaseTradeEvent<'a> {
    #[serde(borrow)]
    trades: Vec<CoinbaseTradeData<'a>>,
}

#[derive(Debug, Deserialize)]
struct CoinbaseTradeData<'a> {
    product_id: &'a str,
    price: &'a str,
    size: &'a str,
    side: &'a str,
    time: &'a str,
}
