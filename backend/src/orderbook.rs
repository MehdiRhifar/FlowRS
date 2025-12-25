use crate::types::{PriceLevel, ClientMessage, TRADING_PAIRS};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Thread-safe order book structure for a single symbol
#[derive(Debug)]
pub struct OrderBook {
    /// Symbol this book represents
    symbol: String,
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
    pub fn new(symbol: &str) -> Self {
        Self {
            symbol: symbol.to_string(),
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
    /// Returns true if update was applied (book changed)
    pub fn apply_update(
        &mut self,
        bids: Vec<(String, String)>,
        asks: Vec<(String, String)>,
        _first_update_id: u64,
        final_update_id: u64,
    ) -> bool {
        if final_update_id <= self.last_update_id {
            return false;
        }

        let mut changed = false;

        for (price_str, qty_str) in bids {
            if let (Ok(price), Ok(qty)) = (price_str.parse(), qty_str.parse()) {
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
            if let (Ok(price), Ok(qty)) = (price_str.parse(), qty_str.parse()) {
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

    /// Trim threshold - trim only when book exceeds 2x target size
    const TRIM_THRESHOLD_MULT: usize = 10;

    /// Trim the book to keep only the best N levels on each side
    /// This prevents accumulation of stale price levels
    /// Optimization: Only trim when size exceeds threshold to reduce allocations
    /// split_off is efficient as it's a native BTreeMap operation
    pub fn trim(&mut self, max_levels: usize) {
        let threshold = max_levels * Self::TRIM_THRESHOLD_MULT;

        // For bids: keep the highest prices (at the end of BTreeMap)
        // split_off returns everything >= key, we want to keep that part
        if self.bids.len() > threshold {
            if let Some(cutoff) = self.bids.keys().nth_back(max_levels - 1).copied() {
                self.bids = self.bids.split_off(&cutoff);
            }
        }

        // For asks: keep the lowest prices (at the beginning of BTreeMap)
        // split_off returns everything >= key, we want to discard that part
        if self.asks.len() > threshold {
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

    /// Calculate total bid depth (sum of all bid quantities)
    pub fn bid_depth(&self) -> Decimal {
        self.bids.values().sum()
    }

    /// Calculate total ask depth (sum of all ask quantities)
    pub fn ask_depth(&self) -> Decimal {
        self.asks.values().sum()
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
            symbol: self.symbol.clone(),
            bids,
            asks,
            spread,
            spread_percent,
            bid_depth: self.bid_depth(),
            ask_depth: self.ask_depth(),
        }
    }
}

/// Multi-symbol order book manager
#[derive(Debug)]
pub struct OrderBookManager {
    books: std::collections::HashMap<String, OrderBook>,
}

impl OrderBookManager {
    pub fn with_symbols(symbols: &[&str]) -> Self {
        let mut books = HashMap::new();
        for symbol in symbols {
            books.insert(symbol.to_string(), OrderBook::new(symbol));
        }
        Self { books }
    }

    pub fn get_mut(&mut self, symbol: &str) -> Option<&mut OrderBook> {
        self.books.get_mut(symbol)
    }

    pub fn get(&self, symbol: &str) -> Option<&OrderBook> {
        self.books.get(symbol)
    }

    pub fn initialized_count(&self) -> usize {
        self.books.values().filter(|b| b.is_initialized()).count()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &OrderBook)> {
        self.books.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut OrderBook)> {
        self.books.iter_mut()
    }
}

impl Default for OrderBookManager {
    fn default() -> Self {
        Self::with_symbols(TRADING_PAIRS)
    }
}

/// Shared multi-symbol order book manager
pub type SharedOrderBookManager = Arc<RwLock<OrderBookManager>>;

pub fn create_shared_orderbook_manager() -> SharedOrderBookManager {
    Arc::new(RwLock::new(OrderBookManager::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orderbook_initialization() {
        let mut book = OrderBook::new("BTCUSDT");
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
            100,
        );

        assert!(book.is_initialized());
        assert_eq!(book.best_bid(), Some(dec!(100.00)));
        assert_eq!(book.best_ask(), Some(dec!(101.00)));
    }

    #[test]
    fn test_spread_calculation() {
        let mut book = OrderBook::new("BTCUSDT");
        book.initialize_from_snapshot(
            vec![
                ("100.00".to_string(), "1.5".to_string()),
                ("99.00".to_string(), "2.0".to_string()),
            ],
            vec![
                ("101.00".to_string(), "1.0".to_string()),
                ("102.00".to_string(), "3.0".to_string()),
            ],
            100,
        );

        let (spread, _spread_percent) = book.spread().unwrap();
        assert_eq!(spread, dec!(1.00));
    }

    #[test]
    fn test_apply_update() {
        let mut book = OrderBook::new("BTCUSDT");
        book.initialize_from_snapshot(
            vec![("100.00".to_string(), "1.5".to_string())],
            vec![("101.00".to_string(), "1.0".to_string())],
            100,
        );

        let changed = book.apply_update(
            vec![("99.00".to_string(), "2.0".to_string())],
            vec![("100.00".to_string(), "3.0".to_string())],
            101,
            102,
        );
        assert!(changed);
        assert_eq!(book.best_bid(), Some(dec!(100.00)));
        assert_eq!(book.best_ask(), Some(dec!(100.00)));
    }

    #[test]
    fn test_depth_calculation() {
        let mut book = OrderBook::new("BTCUSDT");
        book.initialize_from_snapshot(
            vec![
                ("100.00".to_string(), "1.5".to_string()),
                ("99.00".to_string(), "2.0".to_string()),
            ],
            vec![
                ("101.00".to_string(), "1.0".to_string()),
                ("102.00".to_string(), "3.0".to_string()),
            ],
            100,
        );

        let depth = book.bid_depth();
        assert_eq!(depth, dec!(3.5)); // 1.5 + 2.0 = 3.5
        assert_eq!(book.ask_depth(), dec!(4.0)); // 1.0 + 3.0 = 4.0
    }

    #[test]
    fn test_trim() {
        let mut book = OrderBook::new("BTCUSDT");
        // Create 150 bids and 150 asks to exceed the threshold (10 * 10 = 100)
        let bids: Vec<(String, String)> = (0..150)
            .map(|i| (format!("{}.00", 1000 - i), "1.0".to_string()))
            .collect();
        let asks: Vec<(String, String)> = (0..150)
            .map(|i| (format!("{}.00", 1001 + i), "1.0".to_string()))
            .collect();
        
        book.initialize_from_snapshot(bids, asks, 100);

        assert_eq!(book.bids.len(), 150);
        assert_eq!(book.asks.len(), 150);

        book.trim(10);

        // After trim, only 10 levels should remain (threshold is 10x max_levels = 100)
        assert_eq!(book.bids.len(), 10);
        assert_eq!(book.asks.len(), 10);
    }

    #[test]
    fn test_orderbook_manager() {
        let manager = OrderBookManager::with_symbols(TRADING_PAIRS);
        assert_eq!(manager.books.len(), TRADING_PAIRS.len());
        assert!(manager.get("BTCUSDT").is_some());
        assert!(manager.get("ETHUSDT").is_some());
        assert_eq!(manager.initialized_count(), 0); // Not initialized yet
    }
}
