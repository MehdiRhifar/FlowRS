use crate::types::Metrics;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use sysinfo::System;

/// Number of latency samples to keep for percentile calculations
const LATENCY_SAMPLE_SIZE: usize = 2048; // Power of 2 for fast modulo
const LATENCY_SAMPLE_MASK: usize = LATENCY_SAMPLE_SIZE - 1; // For fast modulo via bitwise AND

/// Lock-free ring buffer for latency samples with cached percentiles
/// Uses atomic operations for writing, percentiles computed in background
pub struct LockFreeLatencyBuffer {
    samples: Box<[AtomicU64; LATENCY_SAMPLE_SIZE]>,
    write_index: AtomicUsize,
    count: AtomicU64,
    // Cached percentiles (updated periodically, not on every read)
    cached_p50: AtomicU64,
    cached_p95: AtomicU64,
    cached_p99: AtomicU64,
    // Pre-allocated buffer for percentile calculation (avoids allocation each time)
    scratch_buffer: std::sync::Mutex<Vec<u64>>,
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
            cached_p50: AtomicU64::new(0),
            cached_p95: AtomicU64::new(0),
            cached_p99: AtomicU64::new(0),
            // Pre-allocate buffer once, reuse for each percentile calculation
            scratch_buffer: std::sync::Mutex::new(Vec::with_capacity(LATENCY_SAMPLE_SIZE)),
        }
    }

    /// Record a latency sample - lock-free O(1)
    #[inline(always)]
    pub fn record(&self, latency_us: u64) {
        // Use bitwise AND instead of modulo for power-of-2 sizes
        let index = self.write_index.fetch_add(1, Ordering::Relaxed) & LATENCY_SAMPLE_MASK;
        self.samples[index].store(latency_us, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get cached percentiles - O(1), no allocation
    #[inline(always)]
    pub fn get_cached_percentiles(&self) -> (u64, u64, u64) {
        (
            self.cached_p50.load(Ordering::Relaxed),
            self.cached_p95.load(Ordering::Relaxed),
            self.cached_p99.load(Ordering::Relaxed),
        )
    }

    /// Update cached percentiles - called periodically in background
    /// Uses partial selection (O(n)) instead of full sort (O(n log n))
    /// Reuses pre-allocated buffer to avoid allocation
    pub fn update_percentiles(&self) {
        let count = self.count.load(Ordering::Relaxed) as usize;
        let len = count.min(LATENCY_SAMPLE_SIZE);

        if len < 10 {
            return; // Not enough samples for meaningful percentiles
        }

        // Reuse pre-allocated buffer - no allocation!
        let mut scratch = self.scratch_buffer.lock().unwrap();
        scratch.clear();
        scratch.extend((0..len).map(|i| self.samples[i].load(Ordering::Relaxed)));

        // Use partial selection - O(n) instead of O(n log n)
        // P99 first (highest index), then P95, then P50
        // This order is more efficient because select_nth_unstable partially sorts
        let p99_idx = (len * 99 / 100).min(len - 1);
        let (_, p99, _) = scratch.select_nth_unstable(p99_idx);
        let p99_val = *p99;

        let p95_idx = (len * 95 / 100).min(len - 1);
        let (_, p95, _) = scratch.select_nth_unstable(p95_idx);
        let p95_val = *p95;

        let p50_idx = len / 2;
        let (_, p50, _) = scratch.select_nth_unstable(p50_idx);
        let p50_val = *p50;

        self.cached_p50.store(p50_val, Ordering::Relaxed);
        self.cached_p95.store(p95_val, Ordering::Relaxed);
        self.cached_p99.store(p99_val, Ordering::Relaxed);
    }
}

impl std::fmt::Debug for LockFreeLatencyBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockFreeLatencyBuffer")
            .field("count", &self.count.load(Ordering::Relaxed))
            .finish()
    }
}

/// Cache for system metrics to avoid expensive syscalls every second
/// Updated every 10 seconds in a background task
/// Uses AtomicU64 with f64::to_bits/from_bits for lock-free access
pub struct SystemMetricsCache {
    // Store f64 as u64 bits for atomic access
    memory_used_mb_bits: AtomicU64,
    memory_rss_mb_bits: AtomicU64,
    cpu_usage_percent_bits: AtomicU64,
}

impl SystemMetricsCache {
    pub fn new() -> Self {
        Self {
            memory_used_mb_bits: AtomicU64::new(0.0_f64.to_bits()),
            memory_rss_mb_bits: AtomicU64::new(0.0_f64.to_bits()),
            cpu_usage_percent_bits: AtomicU64::new(0.0_f64.to_bits()),
        }
    }

    #[inline]
    pub fn get(&self) -> (f64, f64, f64) {
        let mem = f64::from_bits(self.memory_used_mb_bits.load(Ordering::Relaxed));
        let rss = f64::from_bits(self.memory_rss_mb_bits.load(Ordering::Relaxed));
        let cpu = f64::from_bits(self.cpu_usage_percent_bits.load(Ordering::Relaxed));
        (mem, rss, cpu)
    }

    pub fn update(&self) {
        tokio::task::block_in_place(|| {
            let mut system = System::new();

            // Get current process memory (RSS = Resident Set Size)
            let pid = sysinfo::Pid::from_u32(std::process::id());
            system.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::Some(&[pid]),
                true,
                sysinfo::ProcessRefreshKind::nothing().with_memory(),
            );

            let (process_mem_mb, process_virt_mb) = system
                .process(pid)
                .map(|p| {
                    let rss = p.memory() as f64 / 1024.0 / 1024.0; // bytes -> MB
                    let virt = p.virtual_memory() as f64 / 1024.0 / 1024.0;
                    (rss, virt)
                })
                .unwrap_or((0.0, 0.0));

            // CPU usage requires two refreshes with a delay, skip for now
            // Just use 0.0 as placeholder (CPU tracking adds overhead)
            let cpu: f64 = 0.0;

            self.memory_used_mb_bits
                .store(process_mem_mb.to_bits(), Ordering::Relaxed);
            self.memory_rss_mb_bits
                .store(process_virt_mb.to_bits(), Ordering::Relaxed);
            self.cpu_usage_percent_bits
                .store(cpu.to_bits(), Ordering::Relaxed);
        });
    }
}

/// Global metrics collector for performance monitoring
pub struct MetricsCollector {
    /// Global message count (all incoming messages)
    global_message_count: AtomicU64,
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
    /// Last reset time for per-second calculations
    last_reset: Arc<std::sync::Mutex<Instant>>,
    /// Previous counts for rate calculation
    last_message_count: AtomicU64,
    last_bytes_received: AtomicU64,
    /// System metrics cache (updated every 10s)
    system_cache: SystemMetricsCache,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            global_message_count: AtomicU64::new(0),
            global_latency_buffer: LockFreeLatencyBuffer::new(),
            global_latency_sum_us: AtomicU64::new(0),
            global_latency_count: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            ws_reconnects: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
            start_time: Instant::now(),
            last_reset: Arc::new(std::sync::Mutex::new(Instant::now())),
            last_message_count: AtomicU64::new(0),
            last_bytes_received: AtomicU64::new(0),
            system_cache: SystemMetricsCache::new(),
        }
    }

    /// Record a message received (all types)
    #[inline]
    pub fn record_message(&self) {
        self.global_message_count.fetch_add(1, Ordering::Relaxed);
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

    /// Record latency from Instant (micro_sec)
    #[inline]
    pub fn record_latency(&self, start: Instant) {
        let latency_us = start.elapsed().as_micros() as u64;
        self.global_latency_sum_us
            .fetch_add(latency_us, Ordering::Relaxed);
        self.global_latency_count.fetch_add(1, Ordering::Relaxed);
        self.global_latency_buffer.record(latency_us);
    }

    /// Compute and return current metrics
    pub fn compute_metrics(&self) -> Metrics {
        let now = Instant::now();
        let mut last_reset = self.last_reset.lock().unwrap();
        let elapsed_secs = last_reset.elapsed().as_secs_f64();

        let current_messages = self.global_message_count.load(Ordering::Relaxed);
        let current_bytes = self.bytes_received.load(Ordering::Relaxed);

        let prev_messages = self
            .last_message_count
            .swap(current_messages, Ordering::Relaxed);
        let prev_bytes = self
            .last_bytes_received
            .swap(current_bytes, Ordering::Relaxed);

        let messages_per_second = if elapsed_secs > 0.0 {
            ((current_messages - prev_messages) as f64 / elapsed_secs) as u64
        } else {
            0
        };

        let bytes_per_second = if elapsed_secs > 0.0 {
            ((current_bytes - prev_bytes) as f64 / elapsed_secs) as u64
        } else {
            0
        };

        let (latency_avg_us, latency_p50_us, latency_p95_us, latency_p99_us) = {
            let latency_sum = self.global_latency_sum_us.swap(0, Ordering::Relaxed);
            let latency_count = self.global_latency_count.swap(0, Ordering::Relaxed);
            let avg = if latency_count > 0 {
                latency_sum as f64 / latency_count as f64
            } else {
                0.0
            };

            // Use cached percentiles - O(1), no allocation
            let (p50, p95, p99) = self.global_latency_buffer.get_cached_percentiles();
            (avg, p50, p95, p99)
        };

        let (memory_used_mb, memory_rss_mb, cpu_usage_percent) = self.system_cache.get();

        let active_connections = self.active_connections.load(Ordering::Relaxed) as u32;
        let websocket_reconnects = self.ws_reconnects.load(Ordering::Relaxed);

        *last_reset = now;

        Metrics {
            messages_per_second,
            bytes_per_second,
            latency_avg_us,
            latency_p50_us,
            latency_p95_us,
            latency_p99_us,
            total_messages: current_messages,
            uptime_seconds: self.start_time.elapsed().as_secs(),
            memory_used_mb,
            memory_rss_mb,
            cpu_usage_percent,
            active_connections,
            websocket_reconnects,
            bytes_received: current_bytes,
        }
    }

    /// Update system metrics (called every 10 seconds)
    pub fn update_system_metrics(&self) {
        self.system_cache.update();
    }

    /// Update latency percentiles (called periodically in background)
    pub fn update_latency_percentiles(&self) {
        self.global_latency_buffer.update_percentiles();
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
