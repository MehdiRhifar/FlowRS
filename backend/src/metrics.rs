use crate::types::Metrics;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use sysinfo::System;
use tokio::sync::RwLock;

/// Metrics collector for performance monitoring
pub struct MetricsCollector {
    /// Counter for messages received from Binance
    message_count: AtomicU64,
    /// Counter for book updates
    update_count: AtomicU64,
    /// Sum of latencies in microseconds for averaging
    latency_sum_us: AtomicU64,
    /// Number of latency samples
    latency_count: AtomicU64,
    /// Start time for uptime calculation
    start_time: Instant,
    /// System info for memory stats
    system: RwLock<System>,
    /// Process ID for memory lookup
    pid: sysinfo::Pid,
    /// Last computed metrics (cached)
    last_metrics: RwLock<Metrics>,
    /// Last reset time for per-second calculations
    last_reset: RwLock<Instant>,
    /// Saved counts from last reset
    last_message_count: AtomicU64,
    last_update_count: AtomicU64,
}

impl MetricsCollector {
    pub fn new() -> Self {
        let pid = sysinfo::Pid::from_u32(std::process::id());
        Self {
            message_count: AtomicU64::new(0),
            update_count: AtomicU64::new(0),
            latency_sum_us: AtomicU64::new(0),
            latency_count: AtomicU64::new(0),
            start_time: Instant::now(),
            system: RwLock::new(System::new()),
            pid,
            last_metrics: RwLock::new(Metrics::default()),
            last_reset: RwLock::new(Instant::now()),
            last_message_count: AtomicU64::new(0),
            last_update_count: AtomicU64::new(0),
        }
    }

    /// Record a message received from Binance
    pub fn record_message(&self) {
        self.message_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an order book update
    pub fn record_update(&self) {
        self.update_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record processing latency in microseconds
    pub fn record_latency_us(&self, latency_us: u64) {
        self.latency_sum_us.fetch_add(latency_us, Ordering::Relaxed);
        self.latency_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record processing latency from an Instant
    pub fn record_latency(&self, start: Instant) {
        let elapsed = start.elapsed();
        self.record_latency_us(elapsed.as_micros() as u64);
    }

    /// Compute and return current metrics
    /// This should be called periodically (e.g., every second)
    pub async fn compute_metrics(&self) -> Metrics {
        let now = Instant::now();
        let mut last_reset = self.last_reset.write().await;
        let elapsed_secs = last_reset.elapsed().as_secs_f64();

        // Get current counts
        let current_messages = self.message_count.load(Ordering::Relaxed);
        let current_updates = self.update_count.load(Ordering::Relaxed);

        // Get previous counts
        let prev_messages = self.last_message_count.load(Ordering::Relaxed);
        let prev_updates = self.last_update_count.load(Ordering::Relaxed);

        // Calculate per-second rates
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

        // Calculate average latency
        let latency_sum = self.latency_sum_us.swap(0, Ordering::Relaxed);
        let latency_count = self.latency_count.swap(0, Ordering::Relaxed);
        let latency_avg_ms = if latency_count > 0 {
            (latency_sum as f64 / latency_count as f64) / 1000.0
        } else {
            0.0
        };

        // Get memory usage
        let memory_used_mb = {
            let mut system = self.system.write().await;
            system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[self.pid]), false);
            system
                .process(self.pid)
                .map(|p| p.memory() as f64 / 1024.0 / 1024.0)
                .unwrap_or(0.0)
        };

        // Update saved counts for next calculation
        self.last_message_count
            .store(current_messages, Ordering::Relaxed);
        self.last_update_count
            .store(current_updates, Ordering::Relaxed);
        *last_reset = now;

        let metrics = Metrics {
            messages_per_second,
            latency_avg_ms,
            updates_per_second,
            uptime_seconds: self.start_time.elapsed().as_secs(),
            memory_used_mb,
        };

        // Cache the metrics
        *self.last_metrics.write().await = metrics.clone();

        metrics
    }

    /// Get the last computed metrics without recomputing
    pub async fn get_cached_metrics(&self) -> Metrics {
        self.last_metrics.read().await.clone()
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
