<script setup lang="ts">
import { computed } from 'vue'
import type { BookUpdate } from '../types'
import { getExchangeColor } from '../constants'

const props = defineProps<{
  books: BookUpdate[]
  symbol?: string
}>()

// Extract base asset from symbol (e.g., "BTCUSDT" -> "BTC")
const baseAsset = computed(() => {
  if (!props.symbol) return 'Asset'
  return props.symbol.replace('USDT', '')
})

interface AggregatedLevel {
  price: string
  quantity: string
  exchanges: Array<{ name: string, quantity: string }>
}

// Aggregate bids from all exchanges
const aggregatedBids = computed((): AggregatedLevel[] => {
  const priceMap = new Map<string, Array<{ name: string, quantity: string }>>()

  for (const book of props.books) {
    for (const bid of book.bids) {
      if (!priceMap.has(bid.price)) {
        priceMap.set(bid.price, [])
      }
      priceMap.get(bid.price)!.push({
        name: book.exchange,
        quantity: bid.quantity
      })
    }
  }

  // Convert to array and sort by price (descending for bids)
  return Array.from(priceMap.entries())
    .map(([price, exchanges]) => {
      const totalQty = exchanges.reduce((sum, ex) => sum + parseFloat(ex.quantity), 0)
      return {
        price,
        quantity: totalQty.toString(),
        exchanges
      }
    })
    .sort((a, b) => parseFloat(b.price) - parseFloat(a.price))
    .slice(0, 15) // Top 15 levels
})

// Aggregate asks from all exchanges
const aggregatedAsks = computed((): AggregatedLevel[] => {
  const priceMap = new Map<string, Array<{ name: string, quantity: string }>>()

  for (const book of props.books) {
    for (const ask of book.asks) {
      if (!priceMap.has(ask.price)) {
        priceMap.set(ask.price, [])
      }
      priceMap.get(ask.price)!.push({
        name: book.exchange,
        quantity: ask.quantity
      })
    }
  }

  // Convert to array and sort by price (ascending for asks)
  return Array.from(priceMap.entries())
    .map(([price, exchanges]) => {
      const totalQty = exchanges.reduce((sum, ex) => sum + parseFloat(ex.quantity), 0)
      return {
        price,
        quantity: totalQty.toString(),
        exchanges
      }
    })
    .sort((a, b) => parseFloat(a.price) - parseFloat(b.price))
    .slice(0, 15) // Top 15 levels
})

// Calculate max quantity for volume bar scaling
const maxQuantity = computed(() => {
  const allQuantities = [
    ...aggregatedBids.value.map(l => parseFloat(l.quantity)),
    ...aggregatedAsks.value.map(l => parseFloat(l.quantity)),
  ]
  return Math.max(...allQuantities, 1)
})

// Calculate best bid/ask across all exchanges
const bestBid = computed(() => {
  if (aggregatedBids.value.length === 0) return null
  return aggregatedBids.value[0].price
})

const bestAsk = computed(() => {
  if (aggregatedAsks.value.length === 0) return null
  return aggregatedAsks.value[0].price
})

const spread = computed(() => {
  if (!bestBid.value || !bestAsk.value) return null
  const spreadVal = parseFloat(bestAsk.value) - parseFloat(bestBid.value)
  const mid = (parseFloat(bestBid.value) + parseFloat(bestAsk.value)) / 2
  const spreadPct = (spreadVal / mid) * 100
  return { value: spreadVal, percent: spreadPct }
})

function formatPrice(price: string): string {
  return parseFloat(price).toLocaleString('en-US', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 20,
  })
}

function formatQuantity(qty: string): string {
  return parseFloat(qty).toFixed(4)
}

function getBarWidth(quantity: string): string {
  const pct = (parseFloat(quantity) / maxQuantity.value) * 100
  return `${Math.min(pct, 100)}%`
}
</script>

<template>
  <div class="orderbook">
    <div class="header">
      <h2>Multi-Exchange Order Book</h2>
      <div v-if="books.length > 0" class="exchanges-info">
        <span
          v-for="book in books"
          :key="book.exchange"
          class="exchange-badge"
          :style="{ backgroundColor: getExchangeColor(book.exchange) }"
        >
          {{ book.exchange }}
        </span>
      </div>
    </div>

    <div v-if="books.length === 0" class="loading">
      Waiting for {{ symbol || 'data' }}...
    </div>

    <template v-else>
      <!-- Column Headers -->
      <div class="column-headers">
        <span class="header-price">Price (USDT)</span>
        <span class="header-quantity">Quantity ({{ baseAsset }})</span>
        <span class="header-exchanges">Exchanges</span>
      </div>

      <!-- Asks (reversed to show lowest at bottom) -->
      <div class="levels asks">
        <div
          v-for="level in [...aggregatedAsks].reverse()"
          :key="'ask-' + level.price"
          class="level ask"
        >
          <div class="bar ask-bar" :style="{ width: getBarWidth(level.quantity) }"></div>
          <span class="price">{{ formatPrice(level.price) }}</span>
          <span class="quantity">{{ formatQuantity(level.quantity) }}</span>
          <span class="exchanges">
            <span
              v-for="ex in level.exchanges"
              :key="ex.name"
              class="exchange-mini-badge"
              :style="{ backgroundColor: getExchangeColor(ex.name) }"
              :title="`${ex.name}: ${formatQuantity(ex.quantity)}`"
            >
              {{ ex.name.substring(0, 3) }}
            </span>
          </span>
        </div>
      </div>

      <!-- Spread -->
      <div class="spread">
        <span v-if="spread">
          Spread: {{ formatPrice(spread.value.toString()) }} ({{ spread.percent.toFixed(3) }}%)
        </span>
      </div>

      <!-- Bids -->
      <div class="levels bids">
        <div
          v-for="level in aggregatedBids"
          :key="'bid-' + level.price"
          class="level bid"
        >
          <div class="bar bid-bar" :style="{ width: getBarWidth(level.quantity) }"></div>
          <span class="price">{{ formatPrice(level.price) }}</span>
          <span class="quantity">{{ formatQuantity(level.quantity) }}</span>
          <span class="exchanges">
            <span
              v-for="ex in level.exchanges"
              :key="ex.name"
              class="exchange-mini-badge"
              :style="{ backgroundColor: getExchangeColor(ex.name) }"
              :title="`${ex.name}: ${formatQuantity(ex.quantity)}`"
            >
              {{ ex.name.substring(0, 3) }}
            </span>
          </span>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.orderbook {
  background: #1a1a2e;
  border-radius: 8px;
  padding: 20px;
  color: #e0e0e0;
  font-family: 'Monaco', 'Courier New', monospace;
  height: calc(100vh - 40px);
  display: flex;
  flex-direction: column;
}

.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
  padding-bottom: 12px;
  border-bottom: 1px solid #2d2d44;
}

.header h2 {
  margin: 0;
  font-size: 18px;
  color: #fff;
}

.exchanges-info {
  display: flex;
  gap: 8px;
}

.exchange-badge {
  padding: 4px 12px;
  border-radius: 12px;
  font-size: 11px;
  font-weight: 600;
  color: #000;
  text-transform: uppercase;
}

.loading {
  text-align: center;
  padding: 40px;
  color: #888;
  font-size: 14px;
}

.column-headers {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  padding: 8px 12px;
  font-size: 11px;
  color: #888;
  text-transform: uppercase;
  border-bottom: 1px solid #2d2d44;
  margin-bottom: 8px;
}

.header-price {
  text-align: right;
  padding-right: 12px;
}

.header-quantity {
  text-align: right;
  padding-right: 12px;
}

.header-exchanges {
  text-align: center;
}

.levels {
  flex: 1;
  overflow-y: auto;
  scrollbar-width: thin;
  scrollbar-color: #444 #1a1a2e;
}

.levels::-webkit-scrollbar {
  width: 6px;
}

.levels::-webkit-scrollbar-track {
  background: #1a1a2e;
}

.levels::-webkit-scrollbar-thumb {
  background: #444;
  border-radius: 3px;
}

.level {
  position: relative;
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  padding: 6px 12px;
  font-size: 13px;
  cursor: pointer;
  transition: background 0.15s;
}

.level:hover {
  background: rgba(255, 255, 255, 0.05);
}

.bar {
  position: absolute;
  top: 0;
  right: 0;
  height: 100%;
  opacity: 0.2;
  transition: width 0.3s ease;
}

.ask-bar {
  background: #e74c3c;
}

.bid-bar {
  background: #27ae60;
}

.price {
  position: relative;
  z-index: 1;
  text-align: right;
  padding-right: 12px;
}

.ask .price {
  color: #e74c3c;
}

.bid .price {
  color: #27ae60;
}

.quantity {
  position: relative;
  z-index: 1;
  text-align: right;
  padding-right: 12px;
}

.exchanges {
  position: relative;
  z-index: 1;
  display: flex;
  gap: 4px;
  justify-content: center;
  align-items: center;
}

.exchange-mini-badge {
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 9px;
  font-weight: 700;
  color: #000;
  text-transform: uppercase;
  cursor: help;
}

.spread {
  text-align: center;
  padding: 12px;
  font-size: 13px;
  color: #f39c12;
  background: rgba(243, 156, 18, 0.1);
  border-radius: 4px;
  margin: 8px 0;
}
</style>
