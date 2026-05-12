<script setup lang="ts">
/**
 * 匹配文本高亮组件
 *
 * 将搜索匹配位置以高亮方式展示
 */

interface Props {
  /** 原始文本 */
  text: string
  /** 匹配位置数组 [start, end] */
  matchIndices: [number, number][]
}

const props = defineProps<Props>()

interface TextSegment {
  text: string
  highlighted: boolean
}

/**
 * 合并重叠区间并生成文字片段
 */
function getSegments(): TextSegment[] {
  if (!props.matchIndices || props.matchIndices.length === 0) {
    return [{ text: props.text, highlighted: false }]
  }

  // 合并重叠区间
  const sorted = [...props.matchIndices].sort((a, b) => a[0] - b[0])
  const merged: [number, number][] = []
  for (const range of sorted) {
    if (merged.length > 0 && range[0] <= merged[merged.length - 1][1]) {
      merged[merged.length - 1][1] = Math.max(merged[merged.length - 1][1], range[1])
    } else {
      merged.push([...range])
    }
  }

  const segments: TextSegment[] = []
  let pos = 0
  for (const [start, end] of merged) {
    if (pos < start) {
      segments.push({ text: props.text.slice(pos, start), highlighted: false })
    }
    segments.push({ text: props.text.slice(start, end), highlighted: true })
    pos = end
  }
  if (pos < props.text.length) {
    segments.push({ text: props.text.slice(pos), highlighted: false })
  }

  return segments
}
</script>

<template>
  <span class="highlighted-text">
    <template v-for="(segment, index) in getSegments()" :key="index">
      <mark v-if="segment.highlighted" class="highlight">{{ segment.text }}</mark>
      <span v-else>{{ segment.text }}</span>
    </template>
  </span>
</template>

<style scoped>
.highlighted-text {
  display: inline;
}

.highlight {
  background-color: var(--color-accent-light, rgba(59, 130, 246, 0.2));
  color: var(--color-accent, #3b82f6);
  border-radius: 2px;
  padding: 0 1px;
}
</style>
