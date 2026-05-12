<script setup lang="ts">
import { ref, computed, watch, onUnmounted, nextTick } from 'vue'

export interface ContextMenuItem {
  id: string
  label: string
  icon: string
  shortcut?: string
  danger?: boolean
}

interface Props {
  visible: boolean
  items: ContextMenuItem[]
  x: number
  y: number
}

const props = withDefaults(defineProps<Props>(), {
  visible: false,
  items: () => [],
  x: 0,
  y: 0,
})

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'select', item: ContextMenuItem): void
}>()

const menuRef = ref<HTMLDivElement | null>(null)
const adjustedPosition = ref({ x: 0, y: 0 })

const menuStyle = computed(() => ({
  left: `${adjustedPosition.value.x}px`,
  top: `${adjustedPosition.value.y}px`,
}))

async function adjustPosition(): Promise<void> {
  await nextTick()

  const menu = menuRef.value
  if (!menu) {
    adjustedPosition.value = { x: props.x, y: props.y }
    return
  }

  const menuRect = menu.getBoundingClientRect()
  const viewportWidth = window.innerWidth
  const viewportHeight = window.innerHeight

  let x = props.x
  let y = props.y

  if (x + menuRect.width > viewportWidth - 8) {
    x = viewportWidth - menuRect.width - 8
  }
  if (y + menuRect.height > viewportHeight - 8) {
    y = viewportHeight - menuRect.height - 8
  }
  x = Math.max(8, x)
  y = Math.max(8, y)

  adjustedPosition.value = { x, y }
}

function handleItemClick(item: ContextMenuItem): void {
  emit('select', item)
  emit('close')
}

function handleClickOutside(event: MouseEvent): void {
  if (menuRef.value && !menuRef.value.contains(event.target as Node)) {
    emit('close')
  }
}

function handleKeydown(event: KeyboardEvent): void {
  if (event.key === 'Escape') {
    emit('close')
  }
}

watch(
  () => props.visible,
  (newVisible) => {
    if (newVisible) {
      adjustPosition()
      document.addEventListener('click', handleClickOutside)
      document.addEventListener('keydown', handleKeydown)
    } else {
      document.removeEventListener('click', handleClickOutside)
      document.removeEventListener('keydown', handleKeydown)
    }
  },
)

watch(
  () => [props.x, props.y],
  () => {
    if (props.visible) {
      adjustPosition()
    }
  },
)

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
  document.removeEventListener('keydown', handleKeydown)
})
</script>

<template>
  <Teleport to="body">
    <Transition name="context-menu-fade">
      <div
        v-if="visible"
        ref="menuRef"
        class="context-menu"
        :style="menuStyle"
        role="menu"
        @click.stop
        @contextmenu.prevent
      >
        <button
          v-for="item in items"
          :key="item.id"
          class="context-menu-item"
          :class="{ 'is-danger': item.danger }"
          role="menuitem"
          @click="handleItemClick(item)"
        >
          <span class="item-icon">{{ item.icon }}</span>
          <span class="item-label">{{ item.label }}</span>
          <span v-if="item.shortcut" class="item-shortcut">{{ item.shortcut }}</span>
        </button>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.context-menu {
  position: fixed;
  min-width: 160px;
  padding: 4px;
  background: var(--color-bg-elevated, #1e1e2e);
  border: 1px solid var(--color-border, #383850);
  border-radius: 8px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.3);
  z-index: 10000;
  backdrop-filter: blur(8px);
}

.context-menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 8px 12px;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: var(--color-text-primary, #e0e0e0);
  font-size: 13px;
  text-align: left;
  cursor: pointer;
  transition: background-color 0.1s ease;
}

.context-menu-item:hover {
  background: var(--color-surface-muted, rgba(255, 255, 255, 0.08));
}

.context-menu-item:active {
  background: var(--color-surface-strong, rgba(255, 255, 255, 0.12));
}

.context-menu-item.is-danger {
  color: var(--color-error, #ef4444);
}

.item-icon {
  font-size: 14px;
  width: 20px;
  text-align: center;
  flex-shrink: 0;
}

.item-label {
  flex: 1;
}

.item-shortcut {
  color: var(--color-text-tertiary, #666);
  font-size: 11px;
  margin-left: 16px;
}

/* Transition */
.context-menu-fade-enter-active {
  transition: opacity 0.1s ease, transform 0.1s ease;
}

.context-menu-fade-leave-active {
  transition: opacity 0.08s ease;
}

.context-menu-fade-enter-from {
  opacity: 0;
  transform: scale(0.95);
}

.context-menu-fade-leave-to {
  opacity: 0;
}
</style>
