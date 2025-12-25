<script setup lang="ts">
import { computed } from 'vue'
import type { Metrics } from '../types'

const props = defineProps<{
  metrics: Metrics | null
  connected: boolean
}>()

const uptime = computed(() => {
  if (!props.metrics) return '--'
  const seconds = props.metrics.uptime_seconds
  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  const secs = seconds % 60
  
  if (hours > 0) {
    return `${hours}h ${minutes}m`
  } else if (minutes > 0) {
    return `${minutes}m ${secs}s`
  }
  return `${secs}s`
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
    
    <div v-else class="metrics-grid">
      <div class="metric">
        <span class="label">Messages/sec</span>
        <span class="value">{{ metrics.messages_per_second.toLocaleString() }}</span>
      </div>
      
      <div class="metric">
        <span class="label">Updates/sec</span>
        <span class="value">{{ metrics.updates_per_second.toLocaleString() }}</span>
      </div>
      
      <div class="metric">
        <span class="label">Latency</span>
        <span class="value">{{ metrics.latency_avg_ms.toFixed(2) }}ms</span>
      </div>
      
      <div class="metric">
        <span class="label">Uptime</span>
        <span class="value">{{ uptime }}</span>
      </div>
      
      <div class="metric">
        <span class="label">Memory</span>
        <span class="value">{{ metrics.memory_used_mb.toFixed(1) }} MB</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.metrics {
  background: #1a1a2e;
  border-radius: 8px;
  padding: 16px;
}

h2 {
  margin: 0 0 12px 0;
  font-size: 14px;
  color: #888;
  text-transform: uppercase;
  letter-spacing: 1px;
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

.metrics-grid {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.metric {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.label {
  color: #888;
  font-size: 13px;
}

.value {
  font-family: 'Monaco', 'Menlo', monospace;
  font-size: 14px;
  color: #fff;
  font-weight: 500;
}
</style>
