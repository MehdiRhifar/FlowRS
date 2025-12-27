import { ref, onMounted, onUnmounted, computed } from 'vue'
import type { BookUpdate, Trade, Metrics, ServerMessage } from '../types'

const WS_URL = 'ws://localhost:8080/ws'
const MAX_TRADES = 50
const RECONNECT_DELAY = 3000

export function useWebSocket() {
  // Store books per exchange+symbol (key format: "exchange:symbol")
  const books = ref<Record<string, BookUpdate>>({})
  // Store trades per symbol (and all trades combined)
  const allTrades = ref<Trade[]>([])
  const tradesBySymbol = ref<Record<string, Trade[]>>({})
  const metrics = ref<Metrics | null>(null)
  const connected = ref(false)
  const error = ref<string | null>(null)
  const symbols = ref<string[]>([])
  const selectedSymbol = ref<string>('BTCUSDT')
  const enabledExchanges = ref<Set<string>>(new Set(['Binance', 'Bybit']))

  let ws: WebSocket | null = null
  let reconnectTimeout: number | null = null

  // Computed: all books for selected symbol (from all enabled exchanges)
  const symbolBooks = computed(() => {
    const result: BookUpdate[] = []
    for (const bookUpdate of Object.values(books.value)) {
      if (bookUpdate.symbol === selectedSymbol.value && enabledExchanges.value.has(bookUpdate.exchange)) {
        result.push(bookUpdate)
      }
    }
    return result
  })

  // Computed: legacy single book (for backwards compatibility) - returns first book
  const book = computed(() => symbolBooks.value[0] || null)

  // Computed: trades for selected symbol
  const trades = computed(() => tradesBySymbol.value[selectedSymbol.value] || [])

  function connect() {
    if (ws?.readyState === WebSocket.OPEN) return

    error.value = null
    ws = new WebSocket(WS_URL)

    ws.onopen = () => {
      connected.value = true
      error.value = null
      console.log('WebSocket connected')
    }

    ws.onclose = () => {
      connected.value = false
      console.log('WebSocket disconnected, reconnecting...')
      scheduleReconnect()
    }

    ws.onerror = (e) => {
      error.value = 'Connection error'
      console.error('WebSocket error:', e)
    }

    ws.onmessage = (event) => {
      try {
        const message: ServerMessage = JSON.parse(event.data)
        handleMessage(message)
      } catch (e) {
        console.error('Failed to parse message:', e)
      }
    }
  }

  function handleMessage(message: ServerMessage) {
    switch (message.type) {
      case 'symbol_list':
        symbols.value = message.data
        // Initialize trade arrays for each symbol
        for (const symbol of message.data) {
          if (!tradesBySymbol.value[symbol]) {
            tradesBySymbol.value[symbol] = []
          }
        }
        break
        
      case 'book_update':
        // Store with composite key: "exchange:symbol"
        const bookKey = `${message.data.exchange}:${message.data.symbol}`
        books.value[bookKey] = message.data
        break
        
      case 'trade':
        // Add to all trades
        allTrades.value = [message.data, ...allTrades.value].slice(0, MAX_TRADES * 2)
        
        // Add to symbol-specific trades
        const symbol = message.data.symbol
        if (!tradesBySymbol.value[symbol]) {
          tradesBySymbol.value[symbol] = []
        }
        tradesBySymbol.value[symbol] = [message.data, ...tradesBySymbol.value[symbol]].slice(0, MAX_TRADES)
        break
        
      case 'metrics':
        metrics.value = message.data
        break
    }
  }

  function scheduleReconnect() {
    if (reconnectTimeout) return
    reconnectTimeout = window.setTimeout(() => {
      reconnectTimeout = null
      connect()
    }, RECONNECT_DELAY)
  }

  function disconnect() {
    if (reconnectTimeout) {
      clearTimeout(reconnectTimeout)
      reconnectTimeout = null
    }
    if (ws) {
      ws.close()
      ws = null
    }
  }

  function reconnect() {
    disconnect()
    connect()
  }

  function selectSymbol(symbol: string) {
    selectedSymbol.value = symbol
  }

  function toggleExchange(exchange: string) {
    if (enabledExchanges.value.has(exchange)) {
      enabledExchanges.value.delete(exchange)
    } else {
      enabledExchanges.value.add(exchange)
    }
    // Trigger reactivity
    enabledExchanges.value = new Set(enabledExchanges.value)
  }

  function handleVisibilityChange() {
    if (document.visibilityState === 'visible') {
      console.log('Tab became visible, reconnecting WebSocket for fresh state...')
      reconnect()
    }
  }

  onMounted(() => {
    connect()
    document.addEventListener('visibilitychange', handleVisibilityChange)
  })

  onUnmounted(() => {
    disconnect()
    document.removeEventListener('visibilitychange', handleVisibilityChange)
  })

  return {
    // Single symbol (for backwards compatibility)
    book,
    trades,
    // Multi-symbol
    books,
    symbolBooks,
    allTrades,
    tradesBySymbol,
    symbols,
    selectedSymbol,
    selectSymbol,
    // Exchange filtering
    enabledExchanges,
    toggleExchange,
    // Global
    metrics,
    connected,
    error,
  }
}
