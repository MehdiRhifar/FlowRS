<script setup lang="ts">
import { useWebSocket } from './composables/useWebSocket'
import OrderBook from './components/OrderBook.vue'
import RecentTrades from './components/RecentTrades.vue'
import MetricsPanel from './components/MetricsPanel.vue'

const { book, trades, metrics, connected } = useWebSocket()
</script>

<template>
  <div class="app">
    <header>
      <h1>Order Book Visualizer</h1>
      <span class="pair">BTC/USDT</span>
    </header>

    <main>
      <div class="orderbook-section">
        <OrderBook :book="book" />
      </div>

      <div class="sidebar">
        <MetricsPanel :metrics="metrics" :connected="connected" />
        <RecentTrades :trades="trades" />
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
  max-width: 1400px;
  margin: 0 auto;
}

header {
  display: flex;
  align-items: center;
  gap: 16px;
}

h1 {
  font-size: 24px;
  font-weight: 600;
}

.pair {
  background: #3b82f6;
  padding: 4px 12px;
  border-radius: 4px;
  font-size: 14px;
  font-weight: 500;
}

main {
  flex: 1;
  display: grid;
  grid-template-columns: 1fr 350px;
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
}

.sidebar > :last-child {
  flex: 1;
  min-height: 300px;
}

@media (max-width: 900px) {
  main {
    grid-template-columns: 1fr;
  }
  
  .sidebar {
    flex-direction: row;
  }
  
  .sidebar > * {
    flex: 1;
  }
}
</style>
