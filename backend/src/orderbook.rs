use crate::types::{ClientMessage, PriceLevel, ORDERBOOK_DEPTH, TRADING_PAIRS};
use dashmap::DashMap;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;

// Facteurs de précision pour conversion Decimal -> u64
// 8 décimales de précision (suffisant pour crypto)
pub const PRICE_FACTOR: u64 = 100_000_000; // 10^8
pub const QTY_FACTOR: u64 = 100_000_000; // 10^8

/// Structure optimisée pour le cache CPU (16 bytes exactement)
#[derive(Debug, Clone, Copy)]
pub struct Level {
    pub price: u64, // Prix * PRICE_FACTOR
    pub qty: u64,   // Quantité * QTY_FACTOR
}

#[derive(Debug)]
pub struct OrderBook {
    symbol: String,
    exchange: String,
    /// Bids: Trié DESC (Plus haut prix en premier) -> [100, 99, 98]
    bids: Vec<Level>,
    /// Asks: Trié ASC (Plus bas prix en premier) -> [101, 102, 103]
    asks: Vec<Level>,
    last_update_id: u64,
    initialized: bool,
    max_depth: usize,
}

impl OrderBook {
    pub fn new(symbol: &str, exchange: &str) -> Self {
        // On pré-alloue un peu plus que la profondeur max pour éviter les réallocs lors des inserts
        let capacity = ORDERBOOK_DEPTH + 10;
        Self {
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            bids: Vec::with_capacity(capacity),
            asks: Vec::with_capacity(capacity),
            last_update_id: 0,
            initialized: false,
            max_depth: ORDERBOOK_DEPTH,
        }
    }

    /// Helper pour convertir u64 interne -> Decimal externe (prix)
    #[inline(always)]
    fn to_external_price(price: u64) -> Decimal {
        Decimal::from(price) / Decimal::from(PRICE_FACTOR)
    }

    /// Helper pour convertir u64 interne -> Decimal externe (quantité)
    #[inline(always)]
    fn to_external_qty(qty: u64) -> Decimal {
        Decimal::from(qty) / Decimal::from(QTY_FACTOR)
    }

    pub fn initialize_from_snapshot(
        &mut self,
        bids: Vec<(u64, u64)>,
        asks: Vec<(u64, u64)>,
        last_update_id: u64,
    ) {
        self.bids.clear();
        self.asks.clear();

        // Remplissage optimisé - données déjà en u64
        for (price, qty) in bids {
            if qty > 0 {
                self.bids.push(Level { price, qty });
            }
        }
        for (price, qty) in asks {
            if qty > 0 {
                self.asks.push(Level { price, qty });
            }
        }

        // Tri initial (Vital pour que le binary_search fonctionne après)
        // Bids: Descendant (b.cmp(a))
        self.bids.sort_unstable_by(|a, b| b.price.cmp(&a.price));
        // Asks: Ascendant (a.cmp(b))
        self.asks.sort_unstable_by(|a, b| a.price.cmp(&b.price));

        // Trim immédiat
        self.truncate_books();

        self.last_update_id = last_update_id;
        self.initialized = true;
    }

    /// Application optimisée des updates WebSocket
    pub fn apply_update(
        &mut self,
        bids: Vec<(u64, u64)>,
        asks: Vec<(u64, u64)>,
        _first_update_id: u64,
        final_update_id: u64,
    ) -> bool {
        let mut changed = false;

        // --- GESTION DES BIDS (Tri DESC) ---
        for (p_int, q_int) in bids {
            // Bids sont triés DESC, donc on inverse la comparaison pour binary_search
            // On cherche où 'p_int' se trouve par rapport aux éléments existants
            let idx_res = self
                .bids
                .binary_search_by(|level| level.price.cmp(&p_int).reverse());

            match idx_res {
                Ok(idx) => {
                    // Prix trouvé
                    if q_int == 0 {
                        self.bids.remove(idx);
                        changed = true;
                    } else {
                        // Update quantité
                        if self.bids[idx].qty != q_int {
                            self.bids[idx].qty = q_int;
                            changed = true;
                        }
                    }
                }
                Err(idx) => {
                    // Prix non trouvé, 'idx' est la position d'insertion
                    if q_int > 0 {
                        // Optimisation: ne pas insérer si c'est au-delà de la profondeur max
                        if idx < self.max_depth {
                            self.bids.insert(
                                idx,
                                Level {
                                    price: p_int,
                                    qty: q_int,
                                },
                            );
                            changed = true;
                            // Si on dépasse, on retire le dernier (le moins bon bid)
                            if self.bids.len() > self.max_depth {
                                self.bids.pop();
                            }
                        }
                    }
                }
            }
        }

        // --- GESTION DES ASKS (Tri ASC) ---
        for (p_int, q_int) in asks {
            // Asks sont triés ASC, comparaison standard
            let idx_res = self.asks.binary_search_by(|level| level.price.cmp(&p_int));

            match idx_res {
                Ok(idx) => {
                    if q_int == 0 {
                        self.asks.remove(idx);
                        changed = true;
                    } else {
                        if self.asks[idx].qty != q_int {
                            self.asks[idx].qty = q_int;
                            changed = true;
                        }
                    }
                }
                Err(idx) => {
                    if q_int > 0 {
                        if idx < self.max_depth {
                            self.asks.insert(
                                idx,
                                Level {
                                    price: p_int,
                                    qty: q_int,
                                },
                            );
                            changed = true;
                            if self.asks.len() > self.max_depth {
                                self.asks.pop();
                            }
                        }
                    }
                }
            }
        }

        self.last_update_id = final_update_id;
        changed
    }

    /// Garde la taille fixe (redondance de sécurité)
    fn truncate_books(&mut self) {
        if self.bids.len() > self.max_depth {
            self.bids.truncate(self.max_depth);
        }
        if self.asks.len() > self.max_depth {
            self.asks.truncate(self.max_depth);
        }
    }

    pub fn best_bid(&self) -> Option<Decimal> {
        self.bids.first().map(|l| Self::to_external_price(l.price))
    }

    pub fn best_ask(&self) -> Option<Decimal> {
        self.asks.first().map(|l| Self::to_external_price(l.price))
    }

    // ... Le reste (spread, to_client_message) doit juste être adapté pour convertir
    // les u64/f64 internes en Decimal/PriceLevel externes.

    pub fn get_top_levels(&self, n: usize) -> (Vec<PriceLevel>, Vec<PriceLevel>) {
        let bids: Vec<PriceLevel> = self
            .bids
            .iter()
            .take(n)
            .map(|l| PriceLevel {
                price: Self::to_external_price(l.price),
                quantity: Self::to_external_qty(l.qty),
            })
            .collect();

        let asks: Vec<PriceLevel> = self
            .asks
            .iter()
            .take(n)
            .map(|l| PriceLevel {
                price: Self::to_external_price(l.price),
                quantity: Self::to_external_qty(l.qty),
            })
            .collect();

        (bids, asks)
    }

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
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub fn last_update_id(&self) -> u64 {
        self.last_update_id
    }
}

// OrderBookManager reste identique car il utilise juste OrderBook comme une boîte noire.

/// Multi-symbol order book manager
#[derive(Debug)]
pub struct OrderBookManager {
    /// Key format: "exchange:symbol" (e.g., "Binance:BTCUSDT")
    books: DashMap<String, OrderBook>,
}

impl OrderBookManager {
    /// Create a composite key from exchange and symbol
    /// Uses a pre-sized buffer to avoid reallocation
    #[inline(always)]
    fn book_key(exchange: &str, symbol: &str) -> String {
        // Pre-allocate exact size needed: exchange + ":" + symbol
        let mut key = String::with_capacity(exchange.len() + 1 + symbol.len());
        key.push_str(exchange);
        key.push(':');
        key.push_str(symbol);
        key
    }

    pub fn with_symbols(_symbols: &[&str]) -> Self {
        // Start with empty books - they'll be created on-demand per exchange
        Self {
            books: DashMap::new(),
        }
    }

    /// Get or create an order book for the given exchange and symbol
    pub fn get_or_create(
        &self,
        exchange: &str,
        symbol: &str,
    ) -> dashmap::mapref::one::RefMut<'_, String, OrderBook> {
        let key = Self::book_key(exchange, symbol);
        self.books
            .entry(key)
            .or_insert_with(|| OrderBook::new(symbol, exchange))
    }

    pub fn get(
        &self,
        exchange: &str,
        symbol: &str,
    ) -> Option<dashmap::mapref::one::Ref<'_, String, OrderBook>> {
        let key = Self::book_key(exchange, symbol);
        self.books.get(&key)
    }

    pub fn iter(
        &self,
    ) -> dashmap::iter::Iter<'_, String, OrderBook, std::collections::hash_map::RandomState> {
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
