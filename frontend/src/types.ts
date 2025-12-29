export interface PriceLevel {
    price: string
    quantity: string
}

export interface BookUpdate {
    exchange: string
    symbol: string
    bids: PriceLevel[]
    asks: PriceLevel[]
    spread: string
    spread_percent: string
    bid_depth: string
    ask_depth: string
}

export interface Trade {
    exchange: string
    symbol: string
    price: string
    quantity: string
    side: 'buy' | 'sell'
    timestamp: number
}

export interface Metrics {
    // Per-second rates
    messages_per_second: number
    bytes_per_second: number

    // Latency stats (in microseconds)
    latency_avg_us: number
    latency_p50_us: number
    latency_p95_us: number
    latency_p99_us: number

    // Totals
    total_messages: number

    // System stats
    uptime_seconds: number
    memory_used_mb: number
    memory_rss_mb: number
    cpu_usage_percent: number

    // Connection stats
    active_connections: number
    websocket_reconnects: number

    // Throughput
    bytes_received: number
}

export type ServerMessage =
    | { type: 'book_update'; data: BookUpdate }
    | { type: 'trade'; data: Trade }
    | { type: 'metrics'; data: Metrics }
    | { type: 'symbol_list'; data: string[] }
