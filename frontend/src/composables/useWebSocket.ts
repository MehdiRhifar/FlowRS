import { ref, onMounted, onUnmounted } from 'vue'
import type { BookUpdate, Trade, Metrics, ServerMessage } from '../types'

const WS_URL = 'ws://localhost:8080/ws'
const MAX_TRADES = 20
const RECONNECT_DELAY = 3000

export function useWebSocket() {
  const book = ref<BookUpdate | null>(null)
  const trades = ref<Trade[]>([])
  const metrics = ref<Metrics | null>(null)
  const connected = ref(false)
  const error = ref<string | null>(null)

  let ws: WebSocket | null = null
  let reconnectTimeout: number | null = null

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
      case 'book_update':
        book.value = message.data
        break
      case 'trade':
        trades.value = [message.data, ...trades.value].slice(0, MAX_TRADES)
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
    book,
    trades,
    metrics,
    connected,
    error,
  }
}
