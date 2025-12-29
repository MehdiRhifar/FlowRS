/**
 * Exchange brand colors
 * Used consistently across all components
 */
export const EXCHANGE_COLORS: Record<string, string> = {
    'Binance': '#f0b90b',  // Official Binance yellow/gold
    'Bybit': '#f7a600',    // Official Bybit orange
    'OKX': '#00c087',      // Official OKX green
    'Coinbase': '#0052ff', // Official Coinbase blue
    'Kraken': '#5741d9',   // Official Kraken purple
}

/**
 * Get exchange color by name, with fallback
 */
export function getExchangeColor(exchange: string): string {
    return EXCHANGE_COLORS[exchange] || '#888'
}
