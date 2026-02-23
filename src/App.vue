<script setup lang="ts">
import RegulationSearchPanel from '@/components/regulation/RegulationSearchPanel.vue'
</script>

<template>
  <main class="app-root">
    <RegulationSearchPanel />
  </main>
</template>

<style scoped>
.app-root {
  width: 100vw;
  height: 100vh;
  overflow: hidden;
  background: var(--color-bg-primary, #f5f7fa);
}
</style>
