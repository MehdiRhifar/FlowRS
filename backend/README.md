# FlowRS Backend

Real-time cryptocurrency order book aggregator built with Rust and Tokio.

## Architecture Overview

```
┌─────────────────┐     ┌─────────────────┐
│  Binance WS     │────▶│                 │
└─────────────────┘     │                 │
                        │  Exchange       │     ┌──────────────────┐
┌─────────────────┐     │  Manager        │────▶│  OrderBook       │
│  Bybit WS       │────▶│                 │     │  Manager         │
└─────────────────┘     │  (per exchange) │     │  (DashMap)       │
                        └─────────────────┘     └──────────────────┘
                                 │                       │
                                 │                       │
                                 ▼                       ▼
                        ┌─────────────────┐     ┌──────────────────┐
                        │  Broadcast      │────▶│  WebSocket       │
                        │  Channel        │     │  Server          │
                        └─────────────────┘     │  (clients)       │
                                                └──────────────────┘
                                                         │
                                                         ▼
                                                  Frontend Clients
```

## Project Structure

```
backend/
├── src/
│   ├── main.rs              # Entry point, initialization
│   ├── exchanges/
│   │   ├── mod.rs           # Exchange trait & types
│   │   ├── binance.rs       # Binance Futures connector
│   │   ├── bybit.rs         # Bybit connector
│   │   └── manager.rs       # Multi-exchange manager
│   ├── orderbook.rs         # OrderBook data structure (DashMap)
│   ├── server.rs            # WebSocket server for clients
│   ├── metrics.rs           # Performance metrics collector
│   └── types.rs             # Shared types & constants
└── Cargo.toml
```

## Module Breakdown

### `main.rs`
- Application entry point
- Creates shared state (order book manager, metrics, broadcast channel)
- Initializes exchange connectors
- Starts exchange manager and WebSocket server

### `exchanges/`
Multi-exchange support with unified interface.

**Key Components:**
- `Exchange` enum: Identifies exchanges (Binance, Bybit)
- `MarketMessage` enum: Normalized market data (DepthUpdate, Trade)
- `ExchangeConnector` enum: Dispatches to exchange-specific implementations
- `ExchangeManager`: Manages connections with auto-reconnect

**Design Pattern:**
Each exchange connector implements:
- `build_subscription_url()`: WebSocket URL construction
- `parse_message()`: Raw JSON → normalized `MarketMessage`
- `fetch_snapshot()`: REST API initial snapshot
- `get_subscription_message()`: Post-connection subscription (Bybit only)

### `orderbook.rs`
Order book data structure with lock-free concurrent access.

**Key Design Decisions:**

1. **DashMap instead of RwLock<HashMap>**
   - Global `RwLock` caused lock contention between exchanges
   - DashMap provides fine-grained per-shard locking (16 shards)
   - Each exchange writes to different keys without blocking others

2. **BTreeMap for price levels**
   - Sorted by price automatically
   - Efficient best bid/ask queries (O(log n))
   - Fast range-based trimming

3. **Composite keys: "exchange:symbol"**
   - Allows multiple exchanges to maintain separate books for same symbol
   - Example: "Binance:BTCUSDT", "Bybit:BTCUSDT"

**Operations:**
- `initialize_from_snapshot()`: Full orderbook initialization
- `apply_update()`: Incremental updates (delta)
- `trim()`: Keep only top N levels to prevent memory bloat
- `to_client_message()`: Serialize for frontend

### `server.rs`
WebSocket server for frontend clients with throttling.

**Features:**
- Per-client connection handling
- Update throttling (1000ms) to avoid overwhelming clients
- Broadcast channel subscription for market data
- Lag recovery: sends full snapshot if client falls behind
- Ping/pong heartbeat support

**Message Flow:**
1. Client connects → send initial snapshot (all orderbooks + metrics)
2. Subscribe to broadcast channel
3. Throttle orderbook updates (keep only latest per exchange:symbol)
4. Send trades and metrics immediately (no throttling)

### `metrics.rs`
Lock-free performance metrics collection.

**Key Optimizations:**

1. **Lock-Free Latency Buffer**
   - Ring buffer with atomic operations
   - 4096 samples for percentile calculations
   - No locks on write path (hot path)

2. **Atomic Counters**
   - `AtomicU64` for all counters
   - No mutex contention
   - Per-symbol and global metrics

**Metrics Tracked:**
- Messages/updates/trades per second
- Latency (avg, min, max, p50, p95, p99)
- Memory usage (RSS, virtual)
- CPU usage
- WebSocket reconnections
- Per-symbol spread (basis points)

### `types.rs`
Shared types and constants.

**Key Types:**
- `PriceLevel`: Price + quantity for orderbook display
- `Trade`: Individual trade with side/timestamp
- `ClientMessage`: Enum of all messages sent to clients
- `Metrics`: Complete performance snapshot

**Constants:**
- `ORDERBOOK_DEPTH`: 100 levels (in-memory)
- `ORDERBOOK_DISPLAY_DEPTH`: 5 levels (sent to clients)
- `TRADING_PAIRS`: List of tracked symbols

## Data Flow

### 1. Exchange → OrderBook

```
Exchange WS → parse_message() → MarketMessage::DepthUpdate
                                       │
                                       ▼
                        ┌──────────────────────────┐
                        │ Convert Decimal→String   │ (before lock)
                        │ Take DashMap lock        │ (~30µs)
                        │ Apply update + trim      │
                        │ Release lock             │
                        └──────────────────────────┘
                                       │
                                       ▼
                        ┌──────────────────────────┐
                        │ Get read lock            │
                        │ Create client message    │
                        │ Broadcast to clients     │
                        └──────────────────────────┘
```

**Lock Duration Optimization:**
- Decimal→String conversion: ~40µs (done **before** lock)
- Update + trim: ~30µs (under lock)
- Client message creation: ~20µs (read lock only)
- Total lock hold time: ~30µs instead of ~100µs

### 2. OrderBook → Clients

```
Broadcast Channel → Client handler
                         │
                         ▼
              ┌──────────────────────┐
              │ Orderbook updates?   │
              │   → Add to pending   │ (throttled)
              └──────────────────────┘
                         │
              ┌──────────────────────┐
              │ Trades/Metrics?      │
              │   → Send immediately │ (no throttling)
              └──────────────────────┘
                         │
                         ▼
              ┌──────────────────────┐
              │ Every 1000ms:        │
              │   Send pending       │
              └──────────────────────┘
```

## Performance Characteristics

### Latency (Message Processing)
- **P50**: ~200µs (median case)
- **P95**: ~700ms (95th percentile)
- **P99**: ~1.5ms (99th percentile)

**Breakdown:**
- JSON parsing: ~50-100µs (serde_json)
- Decimal conversion: ~40µs
- OrderBook update: ~30µs (under DashMap lock)
- Client message creation: ~20µs
- Broadcast: ~10µs

### Throughput
- Handles 100-500 messages/second per exchange
- Supports 9+ trading pairs across multiple exchanges
- Memory usage: ~50-100MB RSS

### Key Optimizations
1. **DashMap**: Eliminated global lock contention
2. **Lock duration**: Reduced from ~100µs → ~30µs
3. **Atomic metrics**: Lock-free performance tracking
4. **Client throttling**: Prevents overwhelming slow clients
5. **Trim strategy**: Only trim when book exceeds 10× target size

## Configuration

Edit `src/types.rs`:

```rust
// Number of price levels to store
pub const ORDERBOOK_DEPTH: usize = 100;

// Number of levels to display to clients
pub const ORDERBOOK_DISPLAY_DEPTH: usize = 5;

// Trading pairs to track
pub const TRADING_PAIRS: &[&str] = &[
    "BTCUSDT",
    "ETHUSDT",
    // ...
];
```

Edit `src/main.rs`:

```rust
// WebSocket server address
const SERVER_ADDR: &str = "0.0.0.0:8080";

// Broadcast channel capacity
const BROADCAST_CAPACITY: usize = 4096;
```

Edit `src/server.rs`:

```rust
// Client update throttle interval
const UPDATE_THROTTLE_MS: u64 = 1000;
```

## Running

```bash
# Development
cargo run

# Release (optimized)
cargo run --release

# With debug logging
RUST_LOG=debug cargo run

# With specific module logging
RUST_LOG=flowrs_backend=debug cargo run
```

## Adding a New Exchange

1. **Create connector** in `src/exchanges/your_exchange.rs`:

```rust
pub struct YourExchangeConnector {
    symbols: Vec<String>,
}

impl YourExchangeConnector {
    pub fn new(symbols: Vec<String>) -> Self { /* ... */ }
    pub fn build_subscription_url(&self, symbols: &[&str]) -> String { /* ... */ }
    pub fn parse_message(&self, raw: &str) -> Result<Option<MarketMessage>, Box<dyn Error>> { /* ... */ }
    pub async fn fetch_snapshot(&self, symbol: &str, limit: usize) -> Result<DepthSnapshot, Box<dyn Error>> { /* ... */ }
    pub fn supported_symbols(&self) -> Vec<String> { /* ... */ }
}
```

2. **Add to enum** in `src/exchanges/mod.rs`:

```rust
#[derive(Debug, Clone, Copy)]
pub enum Exchange {
    Binance,
    Bybit,
    YourExchange,  // Add here
}

pub enum ExchangeConnector {
    Binance(BinanceConn),
    Bybit(BybitConn),
    YourExchange(YourExchangeConn),  // Add here
}
```

3. **Update dispatcher methods** in `ExchangeConnector` enum

4. **Initialize in main.rs**:

```rust
let exchange_connectors = vec![
    ExchangeConnector::Binance(BinanceConn::new(symbols.clone())),
    ExchangeConnector::Bybit(BybitConn::new(symbols.clone())),
    ExchangeConnector::YourExchange(YourExchangeConn::new(symbols.clone())),
];
```

## Testing

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Test specific module
cargo test orderbook

# Run exchange-specific tests
cargo test binance
```

## Dependencies

Key crates:
- `tokio`: Async runtime
- `tokio-tungstenite`: WebSocket client
- `dashmap`: Concurrent HashMap
- `rust_decimal`: High-precision decimal math
- `serde`: Serialization
- `reqwest`: HTTP client (REST snapshots)
- `sysinfo`: System metrics
- `tracing`: Structured logging
