export interface PriceLevel {
  price: string
  quantity: string
}

export interface BookUpdate {
  bids: PriceLevel[]
  asks: PriceLevel[]
  spread: string
  spread_percent: string
}

export interface Trade {
  price: string
  quantity: string
  side: 'buy' | 'sell'
  timestamp: number
}

export interface Metrics {
  messages_per_second: number
  latency_avg_ms: number
  updates_per_second: number
  uptime_seconds: number
  memory_used_mb: number
}

export type ServerMessage =
  | { type: 'book_update'; data: BookUpdate }
  | { type: 'trade'; data: Trade }
  | { type: 'metrics'; data: Metrics }
