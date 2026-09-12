<script setup lang="ts">
defineProps<{
  label: string
  value: string | number
  tone: 'purple' | 'green' | 'indigo' | 'blue' | 'amber'
}>()
</script>

<template>
  <div class="metric-item" :class="`metric-${tone}`">
    <span class="metric-icon"><slot /></span>
    <span class="metric-copy"><strong>{{ value }}</strong><small>{{ label }}</small></span>
  </div>
</template>
