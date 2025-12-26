<script setup lang="ts">
import type { Trade } from '../types'

defineProps<{
  trades: Trade[]
  symbol?: string
}>()

// Exchange colors
const exchangeColors: Record<string, string> = {
  'Binance': '#f0b90b',
  'Bybit': '#f7a600',
  'OKX': '#00c087',
}

function formatTime(timestamp: number): string {
  const date = new Date(timestamp)
  return date.toLocaleTimeString('en-US', {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit'
  })
}

function formatPrice(price: string): string {
  return parseFloat(price).toLocaleString('en-US', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })
}

function formatQuantity(qty: string): string {
  return parseFloat(qty).toFixed(4)
}

function getExchangeColor(exchange: string): string {
  return exchangeColors[exchange] || '#888'
}
</script>

<template>
  <div class="trades">
    <h2>Recent Trades <span v-if="symbol" class="symbol-tag">{{ symbol }}</span></h2>
    
    <div v-if="trades.length === 0" class="empty">
      Waiting for trades...
    </div>
    
    <div v-else class="trade-list">
      <div
        v-for="(trade, i) in trades"
        :key="i"
        class="trade"
        :class="trade.side"
      >
        <span class="time">{{ formatTime(trade.timestamp) }}</span>
        <span
          class="exchange-badge-small"
          :style="{ backgroundColor: getExchangeColor(trade.exchange) }"
          :title="trade.exchange"
        >
          {{ trade.exchange.substring(0, 3) }}
        </span>
        <span class="side">{{ trade.side.toUpperCase() }}</span>
        <span class="quantity">{{ formatQuantity(trade.quantity) }}</span>
        <span class="price">@ {{ formatPrice(trade.price) }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.trades {
  background: #1a1a2e;
  border-radius: 8px;
  padding: 16px;
  height: 100%;
  display: flex;
  flex-direction: column;
}

h2 {
  margin: 0 0 16px 0;
  font-size: 14px;
  color: #888;
  text-transform: uppercase;
  letter-spacing: 1px;
  display: flex;
  align-items: center;
  gap: 8px;
}

.symbol-tag {
  font-size: 10px;
  padding: 2px 6px;
  background: #333;
  border-radius: 3px;
  color: #aaa;
}

.empty {
  color: #666;
  text-align: center;
  padding: 20px;
}

.trade-list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.trade {
  display: grid;
  grid-template-columns: auto auto auto 1fr auto;
  gap: 8px;
  padding: 6px 8px;
  font-family: 'Monaco', 'Menlo', monospace;
  font-size: 12px;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.02);
}

.time {
  color: #666;
}

.exchange-badge-small {
  padding: 2px 5px;
  border-radius: 3px;
  font-size: 9px;
  font-weight: 700;
  color: #000;
  text-transform: uppercase;
  cursor: help;
}

.side {
  font-weight: 600;
  width: 36px;
}

.buy .side {
  color: #22c55e;
}

.sell .side {
  color: #ef4444;
}

.quantity {
  color: #ccc;
  text-align: right;
}

.price {
  color: #888;
}
</style>
