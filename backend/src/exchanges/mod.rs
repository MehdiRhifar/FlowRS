/// Multi-exchange connector support

pub mod binance;
pub mod bybit;
pub mod coinbase;
pub mod kraken;
pub mod manager;

use rust_decimal::Decimal;
use std::error::Error;

use crate::types::Trade;

// Re-export main types
pub use manager::ExchangeManager;
pub use binance::BinanceConnector as BinanceConn;
pub use bybit::BybitConnector as BybitConn;
pub use coinbase::CoinbaseConnector as CoinbaseConn;
pub use kraken::KrakenConnector as KrakenConn;

/// Exchange identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Exchange {
    Binance,
    Bybit,
    Coinbase,
    Kraken,
}

impl Exchange {
    pub fn name(&self) -> &'static str {
        match self {
            Exchange::Binance => "Binance",
            Exchange::Bybit => "Bybit",
            Exchange::Coinbase => "Coinbase",
            Exchange::Kraken => "Kraken",
        }
    }

}

/// Normalized market data message from any exchange
#[derive(Debug, Clone)]
pub enum MarketMessage {
    /// Order book depth update
    DepthUpdate {
        exchange: Exchange,
        symbol: String,
        bids: Vec<(Decimal, Decimal)>,
        asks: Vec<(Decimal, Decimal)>,
        update_id: u64,
        is_snapshot: bool, // true for full snapshot, false for delta update
    },
    /// Individual trade
    Trade(Trade),
    /// Exchange-specific message (for debugging)
    #[allow(dead_code)]
    Raw(String),
}

/// Exchange connector enum with static dispatch
#[derive(Clone)]
pub enum ExchangeConnector {
    Binance(BinanceConn),
    Bybit(BybitConn),
    Coinbase(CoinbaseConn),
    Kraken(KrakenConn),
}

impl ExchangeConnector {
    /// Exchange identifier
    pub fn exchange(&self) -> Exchange {
        match self {
            ExchangeConnector::Binance(_) => Exchange::Binance,
            ExchangeConnector::Bybit(_) => Exchange::Bybit,
            ExchangeConnector::Coinbase(_) => Exchange::Coinbase,
            ExchangeConnector::Kraken(_) => Exchange::Kraken,
        }
    }

    /// Build WebSocket subscription URL for the given symbols
    pub fn build_subscription_url(&self, symbols: &[&str]) -> String {
        match self {
            ExchangeConnector::Binance(b) => b.build_subscription_url(symbols),
            ExchangeConnector::Bybit(b) => b.build_subscription_url(symbols),
            ExchangeConnector::Coinbase(c) => c.build_subscription_url(symbols),
            ExchangeConnector::Kraken(k) => k.build_subscription_url(symbols),
        }
    }

    /// Parse a raw WebSocket message into a normalized MarketMessage
    pub fn parse_message(&self, raw: &str) -> Result<Option<MarketMessage>, Box<dyn Error + Send>> {
        match self {
            ExchangeConnector::Binance(b) => b.parse_message(raw),
            ExchangeConnector::Bybit(b) => b.parse_message(raw),
            ExchangeConnector::Coinbase(c) => c.parse_message(raw),
            ExchangeConnector::Kraken(k) => k.parse_message(raw),
        }
    }

    /// Fetch initial order book snapshot via REST API
    /// Returns Ok(None) if the exchange uses WebSocket snapshots instead
    pub async fn fetch_snapshot(
        &self,
        symbol: &str,
        limit: usize,
    ) -> Result<Option<DepthSnapshot>, Box<dyn Error + Send>> {
        match self {
            ExchangeConnector::Binance(b) => b.fetch_snapshot(symbol, limit).await,
            ExchangeConnector::Bybit(b) => b.fetch_snapshot(symbol, limit).await,
            ExchangeConnector::Coinbase(c) => c.fetch_snapshot(symbol, limit).await,
            ExchangeConnector::Kraken(k) => k.fetch_snapshot(symbol, limit).await,
        }
    }

    /// Get the list of supported symbols
    pub fn supported_symbols(&self) -> Vec<String> {
        match self {
            ExchangeConnector::Binance(b) => b.supported_symbols(),
            ExchangeConnector::Bybit(b) => b.supported_symbols(),
            ExchangeConnector::Coinbase(c) => c.supported_symbols(),
            ExchangeConnector::Kraken(k) => k.supported_symbols(),
        }
    }

    /// Get subscription messages to send after WebSocket connection (if needed)
    /// Returns a list of subscription messages to send sequentially
    pub fn get_subscription_messages(&self, symbols: &[&str]) -> Vec<String> {
        match self {
            ExchangeConnector::Binance(_) => vec![], // Binance subscribes via URL
            ExchangeConnector::Bybit(b) => b.get_subscription_messages(symbols),
            ExchangeConnector::Coinbase(c) => c.get_subscription_messages(),
            ExchangeConnector::Kraken(k) => k.get_subscription_messages(),
        }
    }
}

/// Order book snapshot from REST API
#[derive(Debug, Clone)]
pub struct DepthSnapshot {
    pub bids: Vec<(String, String)>,
    pub asks: Vec<(String, String)>,
    pub last_update_id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exchange_names() {
        assert_eq!(Exchange::Binance.name(), "Binance");
        assert_eq!(Exchange::Bybit.name(), "Bybit");
        assert_eq!(Exchange::Coinbase.name(), "Coinbase");
        assert_eq!(Exchange::Kraken.name(), "Kraken");
    }
}
