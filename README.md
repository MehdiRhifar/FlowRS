# FlowRS - Real-Time Order Book Aggregator

A high-performance, multi-exchange order book aggregator achieving **sub-10µs median latency**. Built with Rust and Vue
3.

![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)
![TypeScript](https://img.shields.io/badge/TypeScript-007ACC?style=flat&logo=typescript&logoColor=white)
![Vue.js](https://img.shields.io/badge/Vue.js-35495E?style=flat&logo=vue.js&logoColor=4FC08D)
![WebSocket](https://img.shields.io/badge/WebSocket-010101?style=flat&logo=socket.io&logoColor=white)

## Performance Results

| Metric      | Latency |
|-------------|---------|
| **P50**     | 9µs     |
| **Average** | 20µs    |
| **P95**     | 63µs    |
| **P99**     | 235µs   |

**Throughput:** 500-1000 msg/s across 4 exchanges × 9 symbols (36 concurrent streams)

---

## Low-Latency Engineering

### 1. CPU Cache-Optimized Order Book

Traditional implementations use `BTreeMap<Price, Quantity>` for O(log n) operations. FlowRS uses a **fixed-size Vec**
instead:

```
BTreeMap (Textbook)              Vec (FlowRS)
├─ Pointer chasing               ├─ Contiguous memory block
├─ Scattered heap allocations    ├─ Single allocation
├─ Cache misses on traversal     ├─ Prefetch-friendly
└─ O(log n) but cache-cold       └─ O(n) but L1/L2 cache-hot
```

**Why it wins:** With 25 price levels (200 bytes), the entire book fits in L1 cache. CPUs iterate contiguous memory ~
100x faster than chasing pointers.
The theoretical O(log n) advantage disappears when every node access is a cache miss.

### 2. Fixed-Point Integer Arithmetic

All prices and quantities use `u64` with a 1e8 scale factor instead of `Decimal` or `f64`:

```rust
const SCALE_FACTOR: u64 = 100_000_000; // 8 decimal places

// "97234.56" → 9_723_456_000_000
fn fast_parse_u64(s: &str) -> Option<u64> {
    // Pure integer math, no heap allocation
    // Inline ASCII → digit conversion
}
```

**Benefits:**

- **No floating-point errors** - Exact decimal representation via integer math
- **5-10x faster than Decimal** - Native CPU integer operations
- **8 bytes vs 16 bytes** - Better cache utilization
- **SIMD-friendly** - Vectorizable comparisons

### 3. Lock-Free Metrics Pipeline

Latency tracking uses a ring buffer with atomic operations. Expensive percentile calculation runs in a background task:

```rust
// Hot path: O(1) atomic write
fn record(&self, latency_us: u64) {
    let idx = self.write_index.fetch_add(1, Relaxed) & 0x7FF; // Bitwise AND
    self.samples[idx].store(latency_us, Relaxed);
}

// Background (every 900ms): O(n) partial selection
fn update_percentiles(&self) {
    // select_nth_unstable() instead of full sort
    // Pre-allocated scratch buffer
}
```

**Techniques:**

- `index & MASK` instead of `index % SIZE` - Avoids expensive integer division
- `select_nth_unstable()` - O(n) partial selection vs O(n log n) sort
- Pre-allocated buffers - No allocation in hot path

### 4. Concurrency Model

**DashMap** (sharded concurrent HashMap) instead of `RwLock<HashMap>`:

- 16 independent shards with fine-grained locking
- Multiple exchanges update different symbols without contention
- Lock hold time minimized

**Isolated Exchange Tasks:**

- Each exchange runs in its own Tokio task
- Independent reconnection logic (one failure doesn't affect others)
- Automatic state recovery on disconnect

### Performance Impact

| Optimization           | Before                   | After                       |
|------------------------|--------------------------|-----------------------------|
| Percentile calculation | In hot path (2.26ms P99) | Background task (235µs P99) |
| Price storage          | `Decimal`                | `u64` - 5-10x faster        |
| Order book             | `BTreeMap`               | `Vec` - Cache-friendly      |
| Metrics buffer         | `Mutex<Vec>`             | Lock-free ring buffer       |

---

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                   Exchange Manager                        │
│            (Tokio task per exchange)                      │
└──────────────────────────────────────────────────────────┘
         │           │           │           │
         ▼           ▼           ▼           ▼
   ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐
   │ Binance │ │  Bybit  │ │ Kraken  │ │Coinbase │
   └─────────┘ └─────────┘ └─────────┘ └─────────┘
         │           │           │           │
         └───────────┴─────┬─────┴───────────┘
                           ▼
              ┌────────────────────────┐
              │   OrderBook Manager    │
              │   (DashMap per symbol) │
              └────────────────────────┘
                           │
                           ▼
              ┌────────────────────────┐
              │   Broadcast Channel    │
              └────────────────────────┘
                           │
                           ▼
              ┌────────────────────────┐
              │   WebSocket Clients    │
              └────────────────────────┘
```

### Plugin Architecture

Adding a new exchange requires ~200 lines:

```rust
pub struct NewExchangeConnector {
    symbols: Vec<String>
}

impl NewExchangeConnector {
    pub fn build_subscription_url(&self) -> String { ... }
    pub fn parse_message(&self, raw: &str) -> Option<MarketMessage> { ... }
    pub fn get_subscription_messages(&self) -> Vec<String> { ... }
}
```

---

## Tech Stack

**Backend (Rust)**

- Tokio async runtime
- tokio-tungstenite (WebSocket)
- DashMap (concurrent HashMap)
- serde/serde_json
- jemalloc allocator

**Frontend (Vue 3 + TypeScript)**

- Vite build tool
- Composition API
- Native WebSocket

---

## Quick Start

```bash
# Backend
cd backend && cargo run --release

# Frontend (separate terminal)
cd frontend && npm install && npm run dev
```

Open `http://localhost:5173`

---

## Project Structure

```
FlowRS/
├── backend/src/
│   ├── exchanges/        # Exchange connectors
│   │   ├── binance.rs
│   │   ├── bybit.rs
│   │   ├── kraken.rs
│   │   ├── coinbase.rs
│   │   └── manager.rs    # Orchestration
│   ├── orderbook.rs      # Vec-based order book
│   ├── metrics.rs        # Lock-free metrics
│   ├── server.rs         # WebSocket server
│   └── types.rs          # Shared types
│
└── frontend/src/
    ├── App.vue
    ├── components/
    └── types.ts
```

---

## Key Takeaways

1. **Cache locality beats algorithmic complexity** - O(n) with L1 cache > O(log n) with cache misses
2. **Integers beat decimals** - Fixed-point arithmetic is faster and avoids precision errors
3. **Move work off the hot path** - Background tasks for expensive computations
4. **Avoid allocations** - Pre-allocate buffers, use `Option` over `Box<dyn Error>`
5. **Lock-free when possible** - Atomics for metrics, sharded maps for data

---

**Built with Rust and Vue 3**
