use crate::types::{PriceLevel, ClientMessage, TRADING_PAIRS, ORDERBOOK_DEPTH};
use dashmap::DashMap;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Thread-safe order book structure for a single symbol
#[derive(Debug)]
pub struct OrderBook {
    /// Symbol this book represents
    symbol: String,
    /// Exchange this book is from
    exchange: String,
    /// Bids (buy orders) - price -> quantity
    /// BTreeMap is sorted ascending, so we iterate in reverse for highest price first
    bids: BTreeMap<Decimal, Decimal>,
    /// Asks (sell orders) - price -> quantity
    /// BTreeMap is sorted ascending, lowest price first
    asks: BTreeMap<Decimal, Decimal>,
    /// Last update ID from exchange (for synchronization)
    last_update_id: u64,
    /// Whether the book has been initialized with snapshot
    initialized: bool,
}

impl OrderBook {
    pub fn new(symbol: &str, exchange: &str) -> Self {
        Self {
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
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
    /// Auto-trims when size exceeds 10x threshold (amortized cost)
    pub fn apply_update(
        &mut self,
        bids: Vec<(String, String)>,
        asks: Vec<(String, String)>,
        _first_update_id: u64,
        final_update_id: u64,
    ) -> bool {
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


        self.auto_trim(ORDERBOOK_DEPTH);
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
    /// Optimization: Only trim when size exceeds threshold to reduce allocations
    /// split_off is efficient as it's a native BTreeMap operation
    pub fn auto_trim(&mut self, max_levels: usize) {
        const TRIM_THRESHOLD_MULT: usize = 3;
        let threshold = max_levels * TRIM_THRESHOLD_MULT;

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
            exchange: self.exchange.clone(),
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
    /// Key format: "exchange:symbol" (e.g., "Binance:BTCUSDT")
    books: DashMap<String, OrderBook>,
}

impl OrderBookManager {
    /// Create a composite key from exchange and symbol
    fn book_key(exchange: &str, symbol: &str) -> String {
        format!("{}:{}", exchange, symbol)
    }

    pub fn with_symbols(_symbols: &[&str]) -> Self {
        // Start with empty books - they'll be created on-demand per exchange
        Self {
            books: DashMap::new(),
        }
    }

    /// Get or create an order book for the given exchange and symbol
    pub fn get_or_create(&self, exchange: &str, symbol: &str) -> dashmap::mapref::one::RefMut<'_, String, OrderBook> {
        let key = Self::book_key(exchange, symbol);
        self.books
            .entry(key)
            .or_insert_with(|| OrderBook::new(symbol, exchange))
    }

    pub fn get(&self, exchange: &str, symbol: &str) -> Option<dashmap::mapref::one::Ref<'_, String, OrderBook>> {
        let key = Self::book_key(exchange, symbol);
        self.books.get(&key)
    }

    pub fn iter(&self) -> dashmap::iter::Iter<'_, String, OrderBook, std::collections::hash_map::RandomState> {
        self.books.iter()
    }
}

impl Default for OrderBookManager {
    fn default() -> Self {
        Self::with_symbols(TRADING_PAIRS)
    }
}

/// Shared multi-symbol order book manager
pub type SharedOrderBookManager = Arc<OrderBookManager>;

pub fn create_shared_orderbook_manager() -> SharedOrderBookManager {
    Arc::new(OrderBookManager::default())
}