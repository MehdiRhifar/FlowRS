<script setup lang="ts">
import { computed } from 'vue'
import type { BookUpdate } from '../types'

const props = defineProps<{
  book: BookUpdate | null
}>()

// Calculate max quantity for volume bar scaling
const maxQuantity = computed(() => {
  if (!props.book) return 1
  const allQuantities = [
    ...props.book.bids.map(l => parseFloat(l.quantity)),
    ...props.book.asks.map(l => parseFloat(l.quantity)),
  ]
  return Math.max(...allQuantities, 1)
})

function formatPrice(price: string): string {
  return parseFloat(price).toLocaleString('en-US', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
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
    <h2>Order Book</h2>
    
    <div v-if="!book" class="loading">
      Waiting for data...
    </div>
    
    <template v-else>
      <!-- Asks (reversed to show lowest at bottom) -->
      <div class="levels asks">
        <div
          v-for="(level, i) in [...book.asks].reverse()"
          :key="'ask-' + i"
          class="level ask"
        >
          <div class="bar ask-bar" :style="{ width: getBarWidth(level.quantity) }"></div>
          <span class="price">{{ formatPrice(level.price) }}</span>
          <span class="quantity">{{ formatQuantity(level.quantity) }}</span>
        </div>
      </div>

      <!-- Spread -->
      <div class="spread">
        Spread: {{ formatPrice(book.spread) }} ({{ parseFloat(book.spread_percent).toFixed(3) }}%)
      </div>

      <!-- Bids -->
      <div class="levels bids">
        <div
          v-for="(level, i) in book.bids"
          :key="'bid-' + i"
          class="level bid"
        >
          <div class="bar bid-bar" :style="{ width: getBarWidth(level.quantity) }"></div>
          <span class="price">{{ formatPrice(level.price) }}</span>
          <span class="quantity">{{ formatQuantity(level.quantity) }}</span>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.orderbook {
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
}

.loading {
  color: #666;
  text-align: center;
  padding: 40px;
}

.levels {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow: hidden;
}

.asks {
  justify-content: flex-end;
}

.level {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 16px;
  padding: 4px 8px;
  position: relative;
  font-family: 'Monaco', 'Menlo', monospace;
  font-size: 13px;
}

.bar {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  opacity: 0.2;
  transition: width 0.15s ease-out;
}

.ask-bar {
  background: #ef4444;
}

.bid-bar {
  background: #22c55e;
}

.ask .price {
  color: #ef4444;
}

.bid .price {
  color: #22c55e;
}

.quantity {
  text-align: right;
  color: #ccc;
}

.spread {
  padding: 12px 8px;
  text-align: center;
  color: #888;
  font-size: 12px;
  border-top: 1px solid #333;
  border-bottom: 1px solid #333;
  margin: 8px 0;
}
</style>
