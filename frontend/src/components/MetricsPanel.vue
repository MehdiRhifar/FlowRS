<script setup lang="ts">
import { computed } from 'vue'
import type { Metrics } from '../types'

const props = defineProps<{
  metrics: Metrics | null
  connected: boolean
  selectedSymbol?: string
}>()

const uptime = computed(() => {
  if (!props.metrics) return '--'
  const seconds = props.metrics.uptime_seconds
  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  const secs = seconds % 60
  
  if (hours > 0) {
    return `${hours}h ${minutes}m ${secs}s`
  } else if (minutes > 0) {
    return `${minutes}m ${secs}s`
  }
  return `${secs}s`
})

const formatBytes = (bytes: number): string => {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`
}

const formatLatency = (us: number): string => {
  if (us < 1000) return `${us.toFixed(0)}µs`
  return `${(us / 1000).toFixed(2)}ms`
}

const symbolMetrics = computed(() => {
  if (!props.metrics || !props.selectedSymbol) return null
  return props.metrics.symbols[props.selectedSymbol] || null
})
</script>

<template>
  <div class="metrics">
    <h2>Performance Metrics</h2>
    
    <div class="status" :class="{ connected }">
      <span class="dot"></span>
      {{ connected ? 'Connected' : 'Disconnected' }}
    </div>

    <div v-if="!metrics" class="loading">
      Waiting for metrics...
    </div>
    
    <div v-else class="metrics-content">
      <!-- Global Stats -->
      <div class="section">
        <h3>Throughput</h3>
        <div class="metrics-grid">
          <div class="metric">
            <span class="label">Messages/sec</span>
            <span class="value highlight">{{ metrics.messages_per_second.toLocaleString() }}</span>
          </div>
          <div class="metric">
            <span class="label">Updates/sec</span>
            <span class="value">{{ metrics.updates_per_second.toLocaleString() }}</span>
          </div>
          <div class="metric">
            <span class="label">Trades/sec</span>
            <span class="value">{{ metrics.trades_per_second.toLocaleString() }}</span>
          </div>
          <div class="metric">
            <span class="label">Bandwidth</span>
            <span class="value">{{ formatBytes(metrics.bytes_per_second) }}/s</span>
          </div>
        </div>
      </div>

      <!-- Latency Stats -->
      <div class="section">
        <h3>Latency per message</h3>
        <div class="metrics-grid">
          <div class="metric">
            <span class="label">Avg</span>
            <span class="value">{{ formatLatency(metrics.latency_avg_us) }}</span>
          </div>
          <div class="metric">
            <span class="label">P50</span>
            <span class="value">{{ formatLatency(metrics.latency_p50_us) }}</span>
          </div>
          <div class="metric">
            <span class="label">P95</span>
            <span class="value">{{ formatLatency(metrics.latency_p95_us) }}</span>
          </div>
          <div class="metric">
            <span class="label">P99</span>
            <span class="value highlight-warn">{{ formatLatency(metrics.latency_p99_us) }}</span>
          </div>
          <div class="metric">
            <span class="label">Min</span>
            <span class="value dim">{{ formatLatency(metrics.latency_min_us) }}</span>
          </div>
          <div class="metric">
            <span class="label">Max</span>
            <span class="value dim">{{ formatLatency(metrics.latency_max_us) }}</span>
          </div>
        </div>
      </div>

      <!-- Totals -->
      <div class="section">
        <h3>Totals</h3>
        <div class="metrics-grid">
          <div class="metric">
            <span class="label">Messages</span>
            <span class="value">{{ metrics.total_messages.toLocaleString() }}</span>
          </div>
          <div class="metric">
            <span class="label">Updates</span>
            <span class="value">{{ metrics.total_updates.toLocaleString() }}</span>
          </div>
          <div class="metric">
            <span class="label">Trades</span>
            <span class="value">{{ metrics.total_trades.toLocaleString() }}</span>
          </div>
          <div class="metric">
            <span class="label">Data</span>
            <span class="value">{{ formatBytes(metrics.bytes_received) }}</span>
          </div>
        </div>
      </div>

      <!-- System Stats -->
      <div class="section">
        <h3>System</h3>
        <div class="metrics-grid">
          <div class="metric">
            <span class="label">Uptime</span>
            <span class="value">{{ uptime }}</span>
          </div>
          <div class="metric">
            <span class="label">Memory</span>
            <span class="value">{{ metrics.memory_used_mb.toFixed(1) }} MB</span>
          </div>
          <div class="metric">
            <span class="label">CPU</span>
            <span class="value">{{ metrics.cpu_usage_percent.toFixed(1) }}%</span>
          </div>
          <div class="metric">
            <span class="label">Symbols</span>
            <span class="value">{{ metrics.active_symbols }}</span>
          </div>
          <div class="metric">
            <span class="label">Clients</span>
            <span class="value">{{ metrics.active_connections }}</span>
          </div>
          <div class="metric">
            <span class="label">Reconnects</span>
            <span class="value">{{ metrics.websocket_reconnects }}</span>
          </div>
        </div>
      </div>

      <!-- Selected Symbol Stats -->
      <div v-if="symbolMetrics" class="section symbol-section">
        <h3>{{ selectedSymbol }}</h3>
        <div class="metrics-grid">
          <div class="metric">
            <span class="label">Msg/sec</span>
            <span class="value">{{ symbolMetrics.messages_per_second }}</span>
          </div>
          <div class="metric">
            <span class="label">Trades/sec</span>
            <span class="value">{{ symbolMetrics.trades_per_second }}</span>
          </div>
          <div class="metric">
            <span class="label">Latency</span>
            <span class="value">{{ formatLatency(symbolMetrics.latency_avg_us) }}</span>
          </div>
          <div class="metric">
            <span class="label">Spread</span>
            <span class="value">{{ symbolMetrics.spread_bps?.toFixed(2) || '--' }} bps</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.metrics {
  background: #1a1a2e;
  border-radius: 8px;
  padding: 16px;
  overflow-y: auto;
}

h2 {
  margin: 0 0 12px 0;
  font-size: 14px;
  color: #888;
  text-transform: uppercase;
  letter-spacing: 1px;
}

h3 {
  margin: 0 0 8px 0;
  font-size: 11px;
  color: #666;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.status {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 16px;
  font-size: 12px;
  color: #ef4444;
}

.status.connected {
  color: #22c55e;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: currentColor;
  animation: pulse 2s infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}

.loading {
  color: #666;
  font-size: 13px;
}

.metrics-content {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.section {
  padding-bottom: 12px;
  border-bottom: 1px solid #2a2a3e;
}

.section:last-child {
  border-bottom: none;
  padding-bottom: 0;
}

.symbol-section {
  background: #252538;
  margin: 0 -8px;
  padding: 12px 8px;
  border-radius: 6px;
  border-bottom: none;
}

.metrics-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px 16px;
}

.metric {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.label {
  color: #666;
  font-size: 11px;
}

.value {
  font-family: 'Monaco', 'Menlo', monospace;
  font-size: 12px;
  color: #fff;
  font-weight: 500;
}

.value.highlight {
  color: #3b82f6;
}

.value.highlight-warn {
  color: #f59e0b;
}

.value.dim {
  color: #888;
}
</style>
