use crate::orderbook::SharedOrderBookManager;
use crate::types::{Metrics, SymbolMetrics, TRADING_PAIRS};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use sysinfo::System;
use tokio::sync::RwLock;

/// Number of latency samples to keep for percentile calculations
const LATENCY_SAMPLE_SIZE: usize = 4096; // Power of 2 for fast modulo

/// Lock-free ring buffer for latency samples
/// Uses atomic operations for writing, only needs lock for reading percentiles
pub struct LockFreeLatencyBuffer {
    samples: Box<[AtomicU64; LATENCY_SAMPLE_SIZE]>,
    write_index: AtomicUsize,
    count: AtomicU64,
}

impl LockFreeLatencyBuffer {
    pub fn new() -> Self {
        // Initialize array of AtomicU64
        let samples: Box<[AtomicU64; LATENCY_SAMPLE_SIZE]> = {
            let mut vec = Vec::with_capacity(LATENCY_SAMPLE_SIZE);
            for _ in 0..LATENCY_SAMPLE_SIZE {
                vec.push(AtomicU64::new(0));
            }
            vec.into_boxed_slice().try_into().unwrap()
        };
        
        Self {
            samples,
            write_index: AtomicUsize::new(0),
            count: AtomicU64::new(0),
        }
    }

    /// Record a latency sample - lock-free O(1)
    #[inline]
    pub fn record(&self, latency_us: u64) {
        let index = self.write_index.fetch_add(1, Ordering::Relaxed) % LATENCY_SAMPLE_SIZE;
        self.samples[index].store(latency_us, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get percentiles - requires collecting samples (called infrequently)
    pub fn get_percentiles(&self) -> (u64, u64, u64, u64, u64) {
        let count = self.count.load(Ordering::Relaxed) as usize;
        let len = count.min(LATENCY_SAMPLE_SIZE);
        
        if len == 0 {
            return (0, 0, 0, 0, 0);
        }

        // Collect samples into a vec for sorting
        let mut samples: Vec<u64> = Vec::with_capacity(len);
        for i in 0..len {
            samples.push(self.samples[i].load(Ordering::Relaxed));
        }
        samples.sort_unstable();

        let min = samples[0];
        let max = samples[len - 1];
        let p50 = samples[(len as f64 * 0.50) as usize];
        let p95 = samples[((len as f64 * 0.95) as usize).min(len - 1)];
        let p99 = samples[((len as f64 * 0.99) as usize).min(len - 1)];

        (min, max, p50, p95, p99)
    }
}

impl std::fmt::Debug for LockFreeLatencyBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockFreeLatencyBuffer")
            .field("count", &self.count.load(Ordering::Relaxed))
            .finish()
    }
}

/// Per-symbol metrics collector
#[derive(Debug)]
pub struct SymbolMetricsCollector {
    message_count: AtomicU64,
    trade_count: AtomicU64,
    latency_sum_us: AtomicU64,
    latency_count: AtomicU64,
    last_message_count: AtomicU64,
    last_trade_count: AtomicU64,
}

impl SymbolMetricsCollector {
    pub fn new(_symbol: &str) -> Self {
        Self {
            message_count: AtomicU64::new(0),
            trade_count: AtomicU64::new(0),
            latency_sum_us: AtomicU64::new(0),
            latency_count: AtomicU64::new(0),
            last_message_count: AtomicU64::new(0),
            last_trade_count: AtomicU64::new(0),
        }
    }

    #[inline]
    pub fn record_message(&self) {
        self.message_count.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_trade(&self) {
        self.trade_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record latency - now lock-free, no async needed
    #[inline]
    pub fn record_latency_us(&self, latency_us: u64) {
        self.latency_sum_us.fetch_add(latency_us, Ordering::Relaxed);
        self.latency_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn compute_metrics(&self, elapsed_secs: f64) -> SymbolMetrics {
        let current_messages = self.message_count.load(Ordering::Relaxed);
        let current_trades = self.trade_count.load(Ordering::Relaxed);

        let prev_messages = self.last_message_count.swap(current_messages, Ordering::Relaxed);
        let prev_trades = self.last_trade_count.swap(current_trades, Ordering::Relaxed);

        let messages_per_second = if elapsed_secs > 0.0 {
            ((current_messages - prev_messages) as f64 / elapsed_secs) as u64
        } else {
            0
        };

        let trades_per_second = if elapsed_secs > 0.0 {
            ((current_trades - prev_trades) as f64 / elapsed_secs) as u64
        } else {
            0
        };

        let latency_sum = self.latency_sum_us.swap(0, Ordering::Relaxed);
        let latency_count = self.latency_count.swap(0, Ordering::Relaxed);
        let latency_avg_us = if latency_count > 0 {
            latency_sum as f64 / latency_count as f64
        } else {
            0.0
        };

        SymbolMetrics {
            messages_per_second,
            trades_per_second,
            latency_avg_us,
            spread_bps: None,
        }
    }
}

/// Global metrics collector for performance monitoring
pub struct MetricsCollector {
    /// Per-symbol collectors
    symbol_collectors: HashMap<String, SymbolMetricsCollector>,
    /// Global message count
    global_message_count: AtomicU64,
    /// Global update count
    global_update_count: AtomicU64,
    /// Global trade count
    global_trade_count: AtomicU64,
    /// Global latency samples for percentile calculations - NOW LOCK-FREE
    global_latency_buffer: LockFreeLatencyBuffer,
    /// Global latency sum
    global_latency_sum_us: AtomicU64,
    /// Global latency count
    global_latency_count: AtomicU64,
    /// Bytes received
    bytes_received: AtomicU64,
    /// WebSocket reconnect count
    ws_reconnects: AtomicU64,
    /// Active WebSocket connections
    active_connections: AtomicU64,
    /// Start time for uptime calculation
    start_time: Instant,
    /// System info for resource monitoring
    system: RwLock<System>,
    /// Process ID
    pid: sysinfo::Pid,
    /// Last reset time for per-second calculations
    last_reset: RwLock<Instant>,
    /// Previous counts for rate calculation
    last_message_count: AtomicU64,
    last_update_count: AtomicU64,
    last_trade_count: AtomicU64,
    last_bytes_received: AtomicU64,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self::with_symbols(TRADING_PAIRS)
    }

    pub fn with_symbols(symbols: &[&str]) -> Self {
        let pid = sysinfo::Pid::from_u32(std::process::id());
        let mut symbol_collectors = HashMap::new();
        for symbol in symbols {
            symbol_collectors.insert(symbol.to_string(), SymbolMetricsCollector::new(symbol));
        }

        Self {
            symbol_collectors,
            global_message_count: AtomicU64::new(0),
            global_update_count: AtomicU64::new(0),
            global_trade_count: AtomicU64::new(0),
            global_latency_buffer: LockFreeLatencyBuffer::new(),
            global_latency_sum_us: AtomicU64::new(0),
            global_latency_count: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            ws_reconnects: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
            start_time: Instant::now(),
            system: RwLock::new(System::new()),
            pid,
            last_reset: RwLock::new(Instant::now()),
            last_message_count: AtomicU64::new(0),
            last_update_count: AtomicU64::new(0),
            last_trade_count: AtomicU64::new(0),
            last_bytes_received: AtomicU64::new(0),
        }
    }

    /// Record a message received from Binance (global + per-symbol)
    pub fn record_nb_message(&self, symbol: &str) {
        self.global_message_count.fetch_add(1, Ordering::Relaxed);
        if let Some(collector) = self.symbol_collectors.get(symbol) {
            collector.record_message();
        }
    }

    /// Record an order book update (global only)
    pub fn record_nb_update(&self) {
        self.global_update_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a trade (global + per-symbol)
    pub fn record_trade_for_symbol(&self, symbol: &str) {
        self.global_trade_count.fetch_add(1, Ordering::Relaxed);
        if let Some(collector) = self.symbol_collectors.get(symbol) {
            collector.record_trade();
        }
    }

    /// Record bytes received
    pub fn record_bytes(&self, bytes: u64) {
        self.bytes_received.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record a WebSocket reconnection
    pub fn record_reconnect(&self) {
        self.ws_reconnects.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment active connections
    pub fn increment_connections(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement active connections
    pub fn decrement_connections(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    /// Record latency from Instant for a specific symbol (micro_sec)
    #[inline]
    pub fn record_latency(&self, symbol: &str, start: Instant) {
        let latency_us = start.elapsed().as_micros() as u64;
        self.global_latency_sum_us.fetch_add(latency_us, Ordering::Relaxed);
        self.global_latency_count.fetch_add(1, Ordering::Relaxed);
        self.global_latency_buffer.record(latency_us);
        if let Some(collector) = self.symbol_collectors.get(symbol) {
            collector.record_latency_us(latency_us);
        }
    }

    /// Compute and return current metrics
    pub async fn compute_metrics(&self, orderbook_manager: Option<&SharedOrderBookManager>) -> Metrics {
        let now = Instant::now();
        let mut last_reset = self.last_reset.write().await;
        let elapsed_secs = last_reset.elapsed().as_secs_f64();

        let current_messages = self.global_message_count.load(Ordering::Relaxed);
        let current_updates = self.global_update_count.load(Ordering::Relaxed);
        let current_trades = self.global_trade_count.load(Ordering::Relaxed);
        let current_bytes = self.bytes_received.load(Ordering::Relaxed);

        let prev_messages = self.last_message_count.swap(current_messages, Ordering::Relaxed);
        let prev_updates = self.last_update_count.swap(current_updates, Ordering::Relaxed);
        let prev_trades = self.last_trade_count.swap(current_trades, Ordering::Relaxed);
        let prev_bytes = self.last_bytes_received.swap(current_bytes, Ordering::Relaxed);

        let messages_per_second = if elapsed_secs > 0.0 {
            ((current_messages - prev_messages) as f64 / elapsed_secs) as u64
        } else {
            0
        };

        let updates_per_second = if elapsed_secs > 0.0 {
            ((current_updates - prev_updates) as f64 / elapsed_secs) as u64
        } else {
            0
        };

        let trades_per_second = if elapsed_secs > 0.0 {
            ((current_trades - prev_trades) as f64 / elapsed_secs) as u64
        } else {
            0
        };

        let bytes_per_second = if elapsed_secs > 0.0 {
            ((current_bytes - prev_bytes) as f64 / elapsed_secs) as u64
        } else {
            0
        };

        let (latency_avg_us, latency_min_us, latency_max_us, latency_p50_us, latency_p95_us, latency_p99_us) = {
            let latency_sum = self.global_latency_sum_us.swap(0, Ordering::Relaxed);
            let latency_count = self.global_latency_count.swap(0, Ordering::Relaxed);
            let avg = if latency_count > 0 {
                latency_sum as f64 / latency_count as f64
            } else {
                0.0
            };

            // Use lock-free buffer for percentiles
            let (min, max, p50, p95, p99) = self.global_latency_buffer.get_percentiles();
            (avg, min, max, p50, p95, p99)
        };

        let (memory_used_mb, memory_rss_mb, cpu_usage_percent) = {
            let mut system = self.system.write().await;
            system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[self.pid]), true);
            system
                .process(self.pid)
                .map(|p| {
                    let mem_mb = p.memory() as f64 / 1024.0 / 1024.0;
                    let rss_mb = p.memory() as f64 / 1024.0 / 1024.0;
                    let cpu = p.cpu_usage() as f64;
                    (mem_mb, rss_mb, cpu)
                })
                .unwrap_or((0.0, 0.0, 0.0))
        };

        let active_symbols = self.symbol_collectors.len() as u32;
        let active_connections = self.active_connections.load(Ordering::Relaxed) as u32;
        let websocket_reconnects = self.ws_reconnects.load(Ordering::Relaxed);

        let mut symbols = HashMap::new();
        if let Some(manager) = orderbook_manager {
            for (symbol, collector) in &self.symbol_collectors {
                let symbol_metrics = collector.compute_metrics(elapsed_secs);

                // Calculate spread_bps from order book
                // TODO: Update to handle multi-exchange (aggregate or pick one exchange)
                let spread_bps = if let Some(book_ref) = manager.get("Binance", symbol) {
                    let book = book_ref.value();
                    if let (Some(bid), Some(ask)) = (book.best_bid(), book.best_ask()) {
                        if bid > Decimal::ZERO {
                            let spread = ask - bid;
                            let mid = (bid + ask) / Decimal::TWO;
                            Some((spread / mid * Decimal::from(10000)).to_string().parse::<f64>().unwrap_or(0.0))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                
                symbols.insert(symbol.clone(), SymbolMetrics {
                    spread_bps,
                    ..symbol_metrics
                });
            }
        }

        *last_reset = now;

        Metrics {
            messages_per_second,
            updates_per_second,
            trades_per_second,
            latency_avg_us,
            latency_min_us,
            latency_max_us,
            latency_p50_us,
            latency_p95_us,
            latency_p99_us,
            total_messages: current_messages,
            total_updates: current_updates,
            total_trades: current_trades,
            uptime_seconds: self.start_time.elapsed().as_secs(),
            memory_used_mb,
            memory_rss_mb,
            cpu_usage_percent,
            active_symbols,
            active_connections,
            websocket_reconnects,
            bytes_received: current_bytes,
            bytes_per_second,
            symbols,
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedMetrics = Arc<MetricsCollector>;

pub fn create_shared_metrics() -> SharedMetrics {
    Arc::new(MetricsCollector::new())
}
