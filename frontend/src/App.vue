<script setup lang="ts">
import {useWebSocket} from './composables/useWebSocket'
import OrderBook from './components/OrderBook.vue'
import RecentTrades from './components/RecentTrades.vue'
import MetricsPanel from './components/MetricsPanel.vue'
import {EXCHANGE_COLORS} from './constants'

const {
  symbolBooks,
  trades,
  metrics,
  connected,
  symbols,
  selectedSymbol,
  selectSymbol,
  enabledExchanges,
  toggleExchange
} = useWebSocket()
</script>

<template>
  <div class="app">
    <header>
      <div class="header-left">
        <h1>FlowRS</h1>
        <span class="subtitle">Real-time Order Book Visualizer</span>
      </div>

      <div class="symbol-selector">
        <button
            v-for="symbol in symbols"
            :key="symbol"
            class="symbol-btn"
            :class="{ active: selectedSymbol === symbol }"
            @click="selectSymbol(symbol)"
        >
          {{ symbol.replace('USDT', '') }}
        </button>
      </div>

      <div class="exchange-filters">
        <span class="filter-label">Exchanges:</span>
        <button
            v-for="exchange in ['Binance', 'Bybit', 'Coinbase', 'Kraken']"
            :key="exchange"
            class="exchange-toggle-btn"
            :class="{ active: enabledExchanges.has(exchange) }"
            :style="enabledExchanges.has(exchange) ? {
            background: EXCHANGE_COLORS[exchange],
            borderColor: EXCHANGE_COLORS[exchange],
            color: '#000'
          } : {}"
            @click="toggleExchange(exchange)"
        >
          {{ exchange }}
        </button>
      </div>

      <div class="header-right">
        <span class="pair">{{ selectedSymbol }}</span>
      </div>
    </header>

    <main>
      <div class="orderbook-section">
        <OrderBook :books="symbolBooks" :symbol="selectedSymbol"/>
      </div>

      <div class="sidebar">
        <MetricsPanel :metrics="metrics" :connected="connected"/>
        <RecentTrades :trades="trades" :symbol="selectedSymbol"/>
      </div>
    </main>
  </div>
</template>

<style>
* {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen,
  Ubuntu, Cantarell, sans-serif;
  background: #0f0f1a;
  color: #fff;
  min-height: 100vh;
}

#app {
  min-height: 100vh;
}
</style>

<style scoped>
.app {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  padding: 20px;
  gap: 20px;
  max-width: 1600px;
  margin: 0 auto;
}

header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
}

.header-left {
  display: flex;
  align-items: baseline;
  gap: 12px;
}

h1 {
  font-size: 24px;
  font-weight: 700;
  background: linear-gradient(135deg, #3b82f6, #8b5cf6);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}

.subtitle {
  font-size: 12px;
  color: #666;
}

.symbol-selector {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
  justify-content: center;
}

.symbol-btn {
  padding: 6px 12px;
  border: 1px solid #333;
  border-radius: 4px;
  background: transparent;
  color: #888;
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s ease;
}

.symbol-btn:hover {
  border-color: #555;
  color: #fff;
}

.symbol-btn.active {
  background: #3b82f6;
  border-color: #3b82f6;
  color: #fff;
}

.exchange-filters {
  display: flex;
  align-items: center;
  gap: 8px;
}

.filter-label {
  font-size: 11px;
  color: #666;
  font-weight: 500;
}

.exchange-toggle-btn {
  padding: 6px 12px;
  border: 1px solid #333;
  border-radius: 4px;
  background: transparent;
  color: #888;
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.2s ease;
}

.exchange-toggle-btn:hover {
  border-color: #555;
}

.header-right {
  display: flex;
  align-items: center;
  gap: 12px;
}

.pair {
  background: #3b82f6;
  padding: 6px 14px;
  border-radius: 4px;
  font-size: 13px;
  font-weight: 600;
}

main {
  flex: 1;
  display: grid;
  grid-template-columns: 1fr 380px;
  gap: 20px;
  min-height: 0;
}

.orderbook-section {
  min-height: 600px;
}

.sidebar {
  display: flex;
  flex-direction: column;
  gap: 20px;
  max-height: calc(100vh - 120px);
  overflow: hidden;
}

.sidebar > :first-child {
  flex-shrink: 0;
  max-height: 60%;
  overflow-y: auto;
}

.sidebar > :last-child {
  flex: 1;
  min-height: 200px;
  overflow: hidden;
}

@media (max-width: 1100px) {
  main {
    grid-template-columns: 1fr;
  }

  .sidebar {
    flex-direction: row;
    max-height: none;
  }

  .sidebar > * {
    flex: 1;
    max-height: 400px;
  }
}

@media (max-width: 768px) {
  .symbol-selector {
    order: 3;
    width: 100%;
  }

  .sidebar {
    flex-direction: column;
  }
}
</style>
