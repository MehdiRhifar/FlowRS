# Exchanges Module

This module provides a unified interface for connecting to multiple cryptocurrency exchanges.

## Structure

```
src/exchanges/
├── mod.rs          # Trait definitions + re-exports
├── binance.rs      # Binance Futures connector
├── bybit.rs        # Bybit Linear connector
└── manager.rs      # ExchangeManager (dispatcher)
```

## Quick Start

### Using a Single Exchange

```rust
use flowRS_backend::exchanges::{BinanceConnector, ExchangeConnector};

let connector = BinanceConnector::new(vec!["BTCUSDT".to_string()]);
let snapshot = connector.fetch_snapshot("BTCUSDT", 10).await?;
```

### Using Multiple Exchanges

```rust
use flowRS_backend::exchanges::{ExchangeManager, BinanceConnector, BybitConnector};

let connectors: Vec<Arc<dyn ExchangeConnector>> = vec![
    Arc::new(BinanceConnector::new(symbols.clone())),
    Arc::new(BybitConnector::new(symbols.clone())),
];

let manager = ExchangeManager::new(connectors, orderbook_manager, metrics);
manager.start_all(tx).await;
```

## Adding a New Exchange

1. Create `src/exchanges/your_exchange.rs`
2. Implement `ExchangeConnector` trait
3. Add to `mod.rs`:
   ```rust
   pub mod your_exchange;
   pub use your_exchange::YourExchangeConnector;
   ```
4. Use in `main.rs`:
   ```rust
   Arc::new(YourExchangeConnector::new(symbols.clone()))
   ```

## Files

| File | Purpose |
|------|---------|
| `mod.rs` | Trait definition, types, re-exports |
| `binance.rs` | Binance Futures implementation |
| `bybit.rs` | Bybit Linear implementation |
| `manager.rs` | Multi-exchange dispatcher |
