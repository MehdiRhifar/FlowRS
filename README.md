# Order Book Visualizer - BTC/USDT

Real-time order book visualization with performance metrics.

## Architecture

```
[Binance WebSocket] → [Rust Backend] → [Vue 3 Frontend]
```

## Features

- Real-time order book updates (depth@100ms)
- Live trades stream
- Performance metrics (messages/sec, latency, memory)
- Auto-reconnection handling
- Trim to prevent stale data accumulation
- Volume bars visualization

## Tech Stack

- **Backend**: Rust + Tokio + WebSocket
- **Frontend**: Vue 3 + TypeScript + Vite
- **Data**: Binance WebSocket API

## Setup

### Backend
```bash
cd backend
cargo build
```

### Frontend
```bash
cd frontend
npm install
```

## Run

### Terminal 1 (Backend)
```bash
cd backend
cargo run
```

### Terminal 2 (Frontend)
```bash
cd frontend
npm run dev
```

Open http://localhost:3000
