use crate::types::{ClientMessage, PriceLevel};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Thread-safe order book structure
#[derive(Debug)]
pub struct OrderBook {
    /// Bids (buy orders) - price -> quantity
    /// BTreeMap is sorted ascending, so we iterate in reverse for highest price first
    bids: BTreeMap<Decimal, Decimal>,
    /// Asks (sell orders) - price -> quantity
    /// BTreeMap is sorted ascending, lowest price first
    asks: BTreeMap<Decimal, Decimal>,
    /// Last update ID from Binance (for synchronization)
    last_update_id: u64,
    /// Whether the book has been initialized with snapshot
    initialized: bool,
}

impl OrderBook {
    pub fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_update_id: 0,
            initialized: false,
        }
    }

    /// Initialize from REST API snapshot
    pub fn initialize_from_snapshot(
        &mut self,
        bids: Vec<(String, String)>,
        asks: Vec<(String, String)>,
        last_update_id: u64,
    ) {
        self.bids.clear();
        self.asks.clear();

        for (price_str, qty_str) in bids {
            if let (Ok(price), Ok(qty)) = (price_str.parse(), qty_str.parse()) {
                if qty > dec!(0) {
                    self.bids.insert(price, qty);
                }
            }
        }

        for (price_str, qty_str) in asks {
            if let (Ok(price), Ok(qty)) = (price_str.parse(), qty_str.parse()) {
                if qty > dec!(0) {
                    self.asks.insert(price, qty);
                }
            }
        }

        self.last_update_id = last_update_id;
        self.initialized = true;
        tracing::info!(
            "Order book initialized with {} bids, {} asks, last_update_id: {}",
            self.bids.len(),
            self.asks.len(),
            last_update_id
        );
    }

    /// Check if the book is ready to receive updates
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Get last update ID
    #[allow(dead_code)]
    pub fn last_update_id(&self) -> u64 {
        self.last_update_id
    }

    /// Apply a depth update from WebSocket
    /// Returns true if the update was applied (book changed)
    pub fn apply_update(
        &mut self,
        bids: Vec<(String, String)>,
        asks: Vec<(String, String)>,
        _first_update_id: u64,
        final_update_id: u64,
    ) -> bool {
        // Binance synchronization logic:
        // Drop events where final_update_id <= last_update_id
        if final_update_id <= self.last_update_id {
            return false;
        }

        // For the first update after snapshot:
        // first_update_id <= last_update_id + 1 AND final_update_id >= last_update_id + 1
        // For subsequent updates: first_update_id == last_update_id + 1

        let mut changed = false;

        for (price_str, qty_str) in bids {
            if let (Ok(price), Ok(qty)) = (price_str.parse::<Decimal>(), qty_str.parse::<Decimal>())
            {
                if qty == dec!(0) {
                    if self.bids.remove(&price).is_some() {
                        changed = true;
                    }
                } else if self.bids.insert(price, qty) != Some(qty) {
                    changed = true;
                }
            }
        }

        for (price_str, qty_str) in asks {
            if let (Ok(price), Ok(qty)) = (price_str.parse::<Decimal>(), qty_str.parse::<Decimal>())
            {
                if qty == dec!(0) {
                    if self.asks.remove(&price).is_some() {
                        changed = true;
                    }
                } else if self.asks.insert(price, qty) != Some(qty) {
                    changed = true;
                }
            }
        }

        self.last_update_id = final_update_id;
        changed
    }

    /// Get the best bid price (highest)
    pub fn best_bid(&self) -> Option<Decimal> {
        self.bids.last_key_value().map(|(price, _)| *price)
    }

    /// Get the best ask price (lowest)
    pub fn best_ask(&self) -> Option<Decimal> {
        self.asks.first_key_value().map(|(price, _)| *price)
    }

    /// Trim the book to keep only the best N levels on each side
    /// This prevents accumulation of stale price levels
    pub fn trim(&mut self, max_levels: usize) {
        // For bids: keep the highest prices (at the end of BTreeMap)
        // split_off returns everything >= key, we want to keep that part
        if self.bids.len() > max_levels {
            if let Some(cutoff) = self.bids.keys().nth_back(max_levels - 1).copied() {
                self.bids = self.bids.split_off(&cutoff);
            }
        }

        // For asks: keep the lowest prices (at the beginning of BTreeMap)
        // split_off returns everything >= key, we want to discard that part
        if self.asks.len() > max_levels {
            if let Some(cutoff) = self.asks.keys().nth(max_levels).copied() {
                self.asks.split_off(&cutoff);
            }
        }
    }

    /// Calculate the spread
    pub fn spread(&self) -> Option<(Decimal, Decimal)> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => {
                let spread = ask - bid;
                let mid_price = (bid + ask) / dec!(2);
                let spread_percent = if mid_price > dec!(0) {
                    (spread / mid_price) * dec!(100)
                } else {
                    dec!(0)
                };
                Some((spread, spread_percent))
            }
            _ => None,
        }
    }

    /// Get top N levels for display
    pub fn get_top_levels(&self, n: usize) -> (Vec<PriceLevel>, Vec<PriceLevel>) {
        let bids: Vec<PriceLevel> = self
            .bids
            .iter()
            .rev() // Reverse to get highest prices first
            .take(n)
            .map(|(price, qty)| PriceLevel {
                price: *price,
                quantity: *qty,
            })
            .collect();

        let asks: Vec<PriceLevel> = self
            .asks
            .iter()
            .take(n) // Lowest prices first
            .map(|(price, qty)| PriceLevel {
                price: *price,
                quantity: *qty,
            })
            .collect();

        (bids, asks)
    }

    /// Create a client message with current book state
    pub fn to_client_message(&self, levels: usize) -> ClientMessage {
        let (bids, asks) = self.get_top_levels(levels);
        let (spread, spread_percent) = self.spread().unwrap_or((dec!(0), dec!(0)));

        ClientMessage::BookUpdate {
            bids,
            asks,
            spread,
            spread_percent,
        }
    }
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared order book wrapped for concurrent access
pub type SharedOrderBook = Arc<RwLock<OrderBook>>;

pub fn create_shared_orderbook() -> SharedOrderBook {
    Arc::new(RwLock::new(OrderBook::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orderbook_initialization() {
        let mut book = OrderBook::new();
        assert!(!book.is_initialized());

        book.initialize_from_snapshot(
            vec![
                ("100.00".to_string(), "1.5".to_string()),
                ("99.00".to_string(), "2.0".to_string()),
            ],
            vec![
                ("101.00".to_string(), "1.0".to_string()),
                ("102.00".to_string(), "3.0".to_string()),
            ],
            12345,
        );

        assert!(book.is_initialized());
        assert_eq!(book.best_bid(), Some(dec!(100.00)));
        assert_eq!(book.best_ask(), Some(dec!(101.00)));
    }

    #[test]
    fn test_spread_calculation() {
        let mut book = OrderBook::new();
        book.initialize_from_snapshot(
            vec![("100.00".to_string(), "1.0".to_string())],
            vec![("101.00".to_string(), "1.0".to_string())],
            1,
        );

        let (spread, spread_percent) = book.spread().unwrap();
        assert_eq!(spread, dec!(1.00));
        // spread_percent = (1 / 100.5) * 100 ≈ 0.995%
        assert!(spread_percent > dec!(0.99) && spread_percent < dec!(1.0));
    }

    #[test]
    fn test_apply_update() {
        let mut book = OrderBook::new();
        book.initialize_from_snapshot(
            vec![("100.00".to_string(), "1.0".to_string())],
            vec![("101.00".to_string(), "1.0".to_string())],
            100,
        );

        // Update with new bid level
        let changed = book.apply_update(
            vec![("100.50".to_string(), "2.0".to_string())],
            vec![],
            101,
            101,
        );
        assert!(changed);
        assert_eq!(book.best_bid(), Some(dec!(100.50)));

        // Remove a level (quantity = 0)
        let changed = book.apply_update(
            vec![("100.50".to_string(), "0".to_string())],
            vec![],
            102,
            102,
        );
        assert!(changed);
        assert_eq!(book.best_bid(), Some(dec!(100.00)));
    }

    #[test]
    fn test_trim() {
        let mut book = OrderBook::new();
        book.initialize_from_snapshot(
            vec![
                ("105.00".to_string(), "1.0".to_string()),
                ("104.00".to_string(), "2.0".to_string()),
                ("103.00".to_string(), "3.0".to_string()),
                ("102.00".to_string(), "4.0".to_string()),
                ("101.00".to_string(), "5.0".to_string()),
                ("100.00".to_string(), "6.0".to_string()),
                ("99.00".to_string(), "7.0".to_string()),
                ("98.00".to_string(), "8.0".to_string()),
                ("97.00".to_string(), "9.0".to_string()),
                ("96.00".to_string(), "10.0".to_string()),
            ],
            vec![
                ("106.00".to_string(), "1.0".to_string()),
                ("107.00".to_string(), "2.0".to_string()),
                ("108.00".to_string(), "3.0".to_string()),
                ("109.00".to_string(), "4.0".to_string()),
                ("110.00".to_string(), "5.0".to_string()),
                ("111.00".to_string(), "6.0".to_string()),
                ("112.00".to_string(), "7.0".to_string()),
                ("113.00".to_string(), "8.0".to_string()),
                ("114.00".to_string(), "9.0".to_string()),
                ("115.00".to_string(), "10.0".to_string()),
            ],
            1,
        );

        assert_eq!(book.bids.len(), 10);
        assert_eq!(book.asks.len(), 10);
        assert_eq!(book.best_bid(), Some(dec!(105.00)));
        assert_eq!(book.best_ask(), Some(dec!(106.00)));

        // Trim to 3 levels
        book.trim(3);

        // Should have only 3 levels left
        assert_eq!(book.bids.len(), 3);
        assert_eq!(book.asks.len(), 3);

        // Best bid should be unchanged (top level)
        assert_eq!(book.best_bid(), Some(dec!(105.00)));
        assert_eq!(book.best_ask(), Some(dec!(106.00)));

        // After trimming, only the top 3 should remain
        let (bids, asks) = book.get_top_levels(10);
        assert_eq!(bids.len(), 3);
        assert_eq!(asks.len(), 3);

        // Verify the correct levels remain
        assert_eq!(bids[0].price, dec!(105.00));
        assert_eq!(bids[1].price, dec!(104.00));
        assert_eq!(bids[2].price, dec!(103.00));

        assert_eq!(asks[0].price, dec!(106.00));
        assert_eq!(asks[1].price, dec!(107.00));
        assert_eq!(asks[2].price, dec!(108.00));
    }
}
