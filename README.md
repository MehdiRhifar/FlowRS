# FlowRS - Real-Time Cryptocurrency Order Book Aggregator

A high-performance, multi-exchange order book aggregator built with Rust and Vue 3. Designed to handle real-time market data from multiple cryptocurrency exchanges simultaneously with sub-millisecond latency.

![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)
![TypeScript](https://img.shields.io/badge/TypeScript-007ACC?style=flat&logo=typescript&logoColor=white)
![Vue.js](https://img.shields.io/badge/Vue.js-35495E?style=flat&logo=vue.js&logoColor=4FC08D)
![WebSocket](https://img.shields.io/badge/WebSocket-010101?style=flat&logo=socket.io&logoColor=white)

## 🎯 Overview

FlowRS aggregates real-time order book data from multiple cryptocurrency exchanges (Binance, Bybit, Kraken, Coinbase), normalizes the data streams, and provides a unified WebSocket API for client applications. Built with performance and extensibility as core design principles.

**Live Features:**
- Real-time order book updates from 4 exchanges
- Trade stream aggregation
- Performance metrics (latency, throughput, memory)
- Multi-symbol support (9+ trading pairs)
- Auto-reconnection with state recovery

## 🏗️ Architecture Highlights

### Extensible Multi-Exchange System

The core architecture uses a **plugin-based design pattern** that makes adding new exchanges trivial:

```
┌─────────────────────────────────────────────────────────────┐
│                    Exchange Manager                         │
│  (One task per exchange, isolated reconnection logic)       │
└─────────────────────────────────────────────────────────────┘
           │           │           │           │
           ▼           ▼           ▼           ▼
     ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐
     │ Binance │ │  Bybit  │ │ Kraken  │ │Coinbase │
     │Connector│ │Connector│ │Connector│ │Connector│
     └─────────┘ └─────────┘ └─────────┘ └─────────┘
           │           │           │           │
           └───────────┴───────────┴───────────┘
                       │
                       ▼
           ┌───────────────────────┐
           │   Normalized Stream   │
           │   (MarketMessage)     │
           └───────────────────────┘
                       │
                       ▼
           ┌───────────────────────┐
           │   OrderBook Manager   │
           │   (DashMap - Lock-free)│
           └───────────────────────┘
                       │
                       ▼
           ┌───────────────────────┐
           │  Broadcast Channel    │
           └───────────────────────┘
                       │
                       ▼
           ┌───────────────────────┐
           │   WebSocket Clients   │
           └───────────────────────┘
```

**Key Design Decisions:**

### 1. **Unified Exchange Interface**
Each exchange implements a common trait with 4 core methods:
- `build_subscription_url()` - WebSocket endpoint construction
- `get_subscription_messages()` - Post-connection subscriptions
- `parse_message()` - Raw JSON → normalized `MarketMessage`
- `fetch_snapshot()` - Initial state via REST API

This abstraction allows **adding a new exchange in ~200 lines of code** without modifying any existing logic.

### 2. **Lock-Free Concurrency with DashMap**
Instead of a global `RwLock<HashMap>`, the system uses **DashMap** - a concurrent hashmap with fine-grained locking:
- **Problem**: Global locks caused contention when multiple exchanges updated different symbols
- **Solution**: DashMap provides per-shard locking (16 shards), allowing parallel writes
- **Result**: Eliminated lock contention, each exchange writes independently

### 3. **Optimized Lock Hold Times**
Critical path optimization reduced lock duration by 70%:
- **Before**: Decimal→String conversion (~40µs) + update (~30µs) = **~100µs under lock**
- **After**: Convert before acquiring lock = **~30µs under lock**
- This matters at scale: 1000 msg/sec × 70µs saved = **70ms/sec freed**

### 4. **Smart Memory Management**
Order books use an **amortized trim strategy**:
- Allow growth up to 10× target size (1000 levels)
- Trim only when threshold exceeded
- Uses BTreeMap's `split_off()` for O(log n) trimming
- Avoids O(log n) overhead on every insert

### 5. **Isolated Reconnection Logic**
Each exchange runs in its own Tokio task with independent reconnection:
- One exchange failure doesn't affect others
- Automatic state recovery (order book reset)
- Exponential backoff (5s delay)

## 🚀 Performance Characteristics

**Latency (End-to-End):**
- **P50**: ~200µs (median message processing)
- **P95**: ~700µs (95th percentile)
- **P99**: ~1.5ms (99th percentile)

**Throughput:**
- Handles **500-1000 messages/second**
- Supports **4 exchanges × 9 symbols = 36 concurrent streams**
- Memory footprint: ~50-100MB RSS

**Optimizations:**
- Lock-free metrics collection (atomic counters)
- Client throttling (1000ms aggregation window)
- Zero-copy message broadcasting
- Efficient BTreeMap for sorted price levels

## 🛠️ Tech Stack

### Backend (Rust)
- **Runtime**: Tokio (async/await, multi-threaded)
- **WebSocket**: tokio-tungstenite
- **Concurrency**: DashMap (lock-free concurrent HashMap)
- **Precision Math**: rust_decimal (financial-grade decimal arithmetic)
- **Serialization**: serde + serde_json
- **HTTP Client**: reqwest (REST API snapshots)
- **Metrics**: Custom lock-free atomic counters

### Frontend (TypeScript + Vue 3)
- **Framework**: Vue 3 Composition API
- **Language**: TypeScript
- **Build Tool**: Vite
- **WebSocket**: Native WebSocket API
- **Styling**: Modern CSS with gradients

## 📦 Project Structure

```
FlowRS/
├── backend/                    # Rust backend
│   ├── src/
│   │   ├── exchanges/          # Exchange connectors (extensible)
│   │   │   ├── mod.rs          # Exchange trait + enums
│   │   │   ├── binance.rs      # Binance Futures connector
│   │   │   ├── bybit.rs        # Bybit connector
│   │   │   ├── kraken.rs       # Kraken connector
│   │   │   ├── coinbase.rs     # Coinbase Advanced Trade
│   │   │   └── manager.rs      # Multi-exchange orchestration
│   │   ├── orderbook.rs        # DashMap-based order book
│   │   ├── server.rs           # WebSocket server (client-facing)
│   │   ├── metrics.rs          # Lock-free metrics collection
│   │   └── types.rs            # Shared types & constants
│   └── Cargo.toml
│
└── frontend/                   # Vue 3 frontend
    ├── src/
    │   ├── components/         # Vue components
    │   ├── types.ts            # TypeScript interfaces
    │   └── App.vue             # Main application
    └── package.json
```

## 🔧 Adding a New Exchange

The architecture makes integration seamless. Example for adding "Kraken":

**Step 1:** Create connector (`src/exchanges/kraken.rs`):
```rust
pub struct KrakenConnector {
    symbols: Vec<String>,
}

impl KrakenConnector {
    pub fn build_subscription_url(&self, _symbols: &[&str]) -> String {
        "wss://ws.kraken.com/v2".to_string()
    }

    pub fn parse_message(&self, raw: &str) -> Result<Option<MarketMessage>, Error> {
        // Parse Kraken-specific JSON format
        // Return normalized MarketMessage::DepthUpdate or MarketMessage::Trade
    }

    pub async fn fetch_snapshot(&self, symbol: &str) -> Result<Option<DepthSnapshot>, Error> {
        // Kraken sends snapshots via WebSocket, so return Ok(None)
    }

    pub fn get_subscription_messages(&self) -> Vec<String> {
        // Return subscription JSON for book + trade channels
    }
}
```

**Step 2:** Add to exchange enum (`src/exchanges/mod.rs`):
```rust
pub enum ExchangeConnector {
    Binance(BinanceConnector),
    Bybit(BybitConnector),
    Kraken(KrakenConnector),  // ← Add here
}
```

**Step 3:** Initialize in main (`src/main.rs`):
```rust
let connectors = vec![
    ExchangeConnector::Binance(BinanceConnector::new(symbols.clone())),
    ExchangeConnector::Kraken(KrakenConnector::new(symbols.clone())),  // ← Add here
];
```

That's it! The exchange manager automatically handles connection, reconnection, and data normalization.

## 📊 Data Flow Pipeline

```
Exchange WebSocket
      │
      ▼
Raw JSON Message
      │
      ▼
parse_message() → MarketMessage { exchange, symbol, bids, asks, ... }
      │
      ▼
DashMap.get_or_create("Binance:BTCUSDT")
      │
      ▼
OrderBook.apply_update(bids, asks) [~30µs under lock]
      │
      ▼
Broadcast Channel → All connected clients
      │
      ▼
Client Throttling (1000ms window) → WebSocket
```

## 🚦 Getting Started

### Prerequisites
- Rust 1.70+ ([install](https://www.rust-lang.org/tools/install))
- Node.js 18+ ([install](https://nodejs.org/))

### Backend Setup
```bash
cd backend
cargo build --release
cargo run --release
```
Server starts on `ws://localhost:8080`

### Frontend Setup
```bash
cd frontend
npm install
npm run dev
```
Frontend available at `http://localhost:5173`

### Development Mode
```bash
# Backend with debug logging
RUST_LOG=debug cargo run

# Frontend with hot reload
npm run dev
```

## 🧪 Testing

```bash
# Backend tests
cd backend
cargo test

# Specific module tests
cargo test orderbook
cargo test exchanges::binance
```

## 📈 Metrics & Monitoring

The system tracks comprehensive performance metrics:

**Per-Symbol Metrics:**
- Messages/second
- Updates/second
- Trades/second
- Spread (in basis points)

**System-Wide Metrics:**
- End-to-end latency (P50, P95, P99)
- Memory usage (RSS, Virtual)
- CPU usage
- Reconnection count

All metrics are collected **lock-free** using atomic operations to avoid performance impact.

## 🎓 Key Learnings & Design Patterns

1. **Trade-offs in Concurrency**: When to use locks vs lock-free structures
2. **Extensibility through Abstraction**: Common traits for different exchange APIs
3. **Performance Optimization**: Measuring before optimizing, reducing critical path
4. **Error Handling**: Graceful degradation (one exchange failure ≠ system failure)
5. **Memory Management**: Amortized cleanup strategies
6. **Type Safety**: Rust's type system preventing runtime errors

---

**Built with ❤️ using Rust and Vue 3**
