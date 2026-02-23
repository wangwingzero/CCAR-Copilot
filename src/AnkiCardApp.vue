<script setup lang="ts">
/**
 * Anki 单词卡制作窗口
 *
 * 布局：左侧截图预览 + 右侧单词列表和导入控制
 * 功能：OCR 提取单词 -> 编辑列表 -> 选择牌组 -> 批量导入
 */
import { ref, onMounted, onUnmounted, computed } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { useWordCard } from '@/composables/useWordCard'
import { useTheme } from '@/composables/useTheme'
import type { ImportResult } from '@/composables/useWordCard'

// 初始化主题（跟随全局设置）
useTheme()

// ============================================
// State
// ============================================

const imagePath = ref('')
const imageSrc = ref('')
const ocrText = ref('')
const words = ref<string[]>([])
const newWord = ref('')
const selectedDeck = ref('')
const customDeck = ref('')
const useCustomDeck = ref(false)
const decks = ref<string[]>([])
const isConnected = ref(false)
const isLoading = ref(true)
const importResult = ref<ImportResult | null>(null)
const showResult = ref(false)

const {
  isImporting,
  importProgress,
  importTotal,
  progressPercent,
  error,
  extractEnglishWords,
  importWords,
  checkConnection,
  getDecks,
} = useWordCard()

// 牌组搜索
const deckSearch = ref('')
const showDeckDropdown = ref(false)
const filteredDecks = computed(() => {
  if (!deckSearch.value.trim()) return decks.value
  const keyword = deckSearch.value.toLowerCase()
  return decks.value.filter(d => d.toLowerCase().includes(keyword))
})

function selectDeck(deck: string) {
  selectedDeck.value = deck
  deckSearch.value = ''
  showDeckDropdown.value = false
}

function onDeckSearchFocus() {
  showDeckDropdown.value = true
}

function onDeckSearchBlur() {
  // 延迟关闭，让点击事件有时间触发
  setTimeout(() => { showDeckDropdown.value = false }, 200)
}

/** 获取牌组的简短显示名称 */
function deckDisplayName(deck: string): string {
  const parts = deck.split('::')
  return parts[parts.length - 1]
}

/** 获取牌组的层级路径（不含最后一级） */
function deckParentPath(deck: string): string {
  const parts = deck.split('::')
  if (parts.length <= 1) return ''
  return parts.slice(0, -1).join(' › ')
}

// ============================================
// Computed
// ============================================

const deckName = computed(() => {
  if (useCustomDeck.value && customDeck.value.trim()) {
    return customDeck.value.trim()
  }
  return selectedDeck.value
})

const todayDeck = computed(() => {
  const now = new Date()
  const y = now.getFullYear()
  const m = String(now.getMonth() + 1).padStart(2, '0')
  const d = String(now.getDate()).padStart(2, '0')
  return `000单词${y}-${m}-${d}`
})

const canImport = computed(() =>
  isConnected.value &&
  words.value.length > 0 &&
  deckName.value.length > 0
)

// ============================================
// Methods
// ============================================

function addWord() {
  const w = newWord.value.trim()
  if (w && !words.value.includes(w)) {
    words.value.push(w)
  }
  newWord.value = ''
}

function removeWord(index: number) {
  words.value.splice(index, 1)
}

// 编辑单词
const editingIndex = ref<number | null>(null)
const editingValue = ref('')

function startEditWord(index: number) {
  editingIndex.value = index
  editingValue.value = words.value[index]
}

function finishEditWord(index: number) {
  const trimmed = editingValue.value.trim()
  if (trimmed) {
    words.value[index] = trimmed
  } else {
    words.value.splice(index, 1) // 清空则删除
  }
  editingIndex.value = null
  editingValue.value = ''
}

function cancelEditWord() {
  editingIndex.value = null
  editingValue.value = ''
}

function clearWords() {
  words.value = []
}

async function doImport() {
  if (!canImport.value) return

  const wordList = [...words.value]
  const deck = deckName.value
  const imgPath = imagePath.value || undefined

  // 清空当前单词列表（已提交到后台），用户可以继续添加新单词
  words.value = []
  showResult.value = false
  importSubmitted.value = true

  // 后台导入（不阻塞 UI，窗口保持打开）
  importWords(wordList, deck, imgPath).then(async (result) => {
    importSubmitted.value = false
    if (result) {
      importResult.value = result
      showResult.value = true

      // 发送桌面通知
      try {
        if (Notification.permission === 'granted') {
          new Notification('Anki 导入完成', {
            body: `成功导入 ${result.success_count}/${result.total_count} 个单词到 ${deck}`,
          })
        } else if (Notification.permission !== 'denied') {
          const perm = await Notification.requestPermission()
          if (perm === 'granted') {
            new Notification('Anki 导入完成', {
              body: `成功导入 ${result.success_count}/${result.total_count} 个单词到 ${deck}`,
            })
          }
        }
      } catch {
        console.log('[AnkiCard] 导入完成:', result.success_count, '/', result.total_count)
      }
    }
  }).catch((e) => {
    importSubmitted.value = false
    console.error('[AnkiCard] 导入失败:', e)
  })
}

const importSubmitted = ref(false)

async function minimizeWindow() {
  try { await getCurrentWindow().minimize() } catch { /* ignore */ }
}

async function toggleMaximize() {
  try {
    const win = getCurrentWindow()
    if (await win.isMaximized()) {
      await win.unmaximize()
    } else {
      await win.maximize()
    }
  } catch { /* ignore */ }
}

async function closeWindow() {
  try {
    await getCurrentWindow().close()
  } catch (e) {
    console.error('[AnkiCard] 关闭窗口失败:', e)
    try {
      await getCurrentWindow().destroy()
    } catch {
      // ignore
    }
  }
}

function handleKeyDown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    closeWindow()
  }
}

// ============================================
// Init
// ============================================

/** Tauri 事件取消监听函数 */
let unlistenAnkiInit: (() => void) | null = null

interface AnkiCardInitInfo {
  imagePath?: string
  ocrText?: string
  highlightWords?: string[]
}

/** 后台检查 Anki 连接（不阻塞 UI） */
const isCheckingConnection = ref(false)

async function tryConnect() {
  isCheckingConnection.value = true
  try {
    isConnected.value = await checkConnection()
  } catch (e) {
    console.warn('[AnkiCard] 连接检查失败:', e)
    isConnected.value = false
  }

  if (isConnected.value) {
    try {
      decks.value = await getDecks()
    } catch {
      // 忽略
    }
    // 默认使用日期牌组
    useCustomDeck.value = true
    customDeck.value = todayDeck.value
  }
  isCheckingConnection.value = false
}

/** 处理初始化数据 */
async function handleInitData(info: AnkiCardInitInfo) {
  // 先清空旧数据（窗口复用时必须重置）
  words.value = []
  ocrText.value = ''
  imagePath.value = ''
  imageSrc.value = ''
  showResult.value = false
  importResult.value = null
  editingIndex.value = null

  if (info.imagePath) {
    imagePath.value = info.imagePath
    imageSrc.value = convertFileSrc(info.imagePath)
    console.log('[AnkiCard] 截图已加载:', info.imagePath)
  }

  // 优先使用高亮单词
  if (info.highlightWords && info.highlightWords.length > 0) {
    words.value = [...info.highlightWords]
  } else if (info.ocrText) {
    // 没有高亮单词时，从 OCR 文本提取
    ocrText.value = info.ocrText
    try {
      const extracted = await extractEnglishWords(info.ocrText)
      words.value = extracted
    } catch (e) {
      console.warn('[AnkiCard] 提取单词失败:', e)
    }
  }
}

async function initialize() {
  // 监听事件（用于窗口已存在时的数据更新）
  unlistenAnkiInit = await listen<AnkiCardInitInfo>('anki-card-init', async (event) => {
    await handleInitData(event.payload)
  })

  // 主动拉取初始化数据（比事件更可靠，避免时序问题）
  try {
    const pendingData = await invoke<AnkiCardInitInfo | null>('get_pending_anki_init')
    if (pendingData) {
      console.log('[AnkiCard] 从状态拉取到初始化数据')
      await handleInitData(pendingData)
    }
  } catch (e) {
    console.warn('[AnkiCard] 拉取初始化数据失败:', e)
  }

  // UI 立即可用
  isLoading.value = false

  // 后台检查 Anki 连接
  tryConnect()
}

onMounted(() => {
  initialize()
  document.addEventListener('keydown', handleKeyDown)
})

onUnmounted(() => {
  document.removeEventListener('keydown', handleKeyDown)
  unlistenAnkiInit?.()
  unlistenAnkiInit = null
})
</script>

<template>
  <div class="anki-card-app">
    <!-- 标题栏 -->
    <header class="titlebar" data-tauri-drag-region>
      <span class="titlebar-title" data-tauri-drag-region>Anki 单词卡</span>
      <div class="titlebar-buttons">
        <button class="titlebar-btn" @click="minimizeWindow" title="最小化">─</button>
        <button class="titlebar-btn" @click="toggleMaximize" title="最大化">☐</button>
        <button class="titlebar-btn titlebar-close" @click="closeWindow" title="关闭">✕</button>
      </div>
    </header>

    <!-- 主体 -->
    <div class="main-content">
      <!-- 左侧: 截图预览 -->
      <div class="preview-panel">
        <div v-if="imageSrc" class="image-container">
          <img :src="imageSrc" alt="截图预览" class="preview-image" />
        </div>
        <div v-else class="empty-preview">
          <span>暂无截图</span>
        </div>
      </div>

      <!-- 右侧: 控制面板 -->
      <div class="control-panel">
        <!-- 连接状态 -->
        <div v-if="isCheckingConnection" class="status-bar loading">
          <span class="spinner"></span> 正在连接 Anki...
        </div>
        <div v-else-if="!isConnected" class="status-bar error">
          <span>⚠ 无法连接到 Anki，请确保 Anki 已启动并安装了 AnkiConnect 插件</span>
          <button class="btn-sm" @click="tryConnect">重试</button>
        </div>
        <div v-else class="status-bar success">
          <span>✓ 已连接 Anki</span>
        </div>

        <!-- 单词列表 -->
        <div class="word-list-section">
          <div class="section-header">
            <span class="section-title">单词列表 ({{ words.length }})</span>
            <button v-if="words.length > 0" class="btn-sm btn-danger" @click="clearWords">清空</button>
          </div>

          <div class="word-list">
            <div v-for="(word, index) in words" :key="index" class="word-item" @dblclick="startEditWord(index)">
              <input
                v-if="editingIndex === index"
                v-model="editingValue"
                class="word-edit-input"
                @blur="finishEditWord(index)"
                @keydown.enter="finishEditWord(index)"
                @keydown.escape="cancelEditWord"
                ref="wordEditInput"
                autofocus
              />
              <span v-else class="word-text">{{ word }}</span>
              <button class="word-remove" @click="removeWord(index)" title="删除">✕</button>
            </div>
            <div v-if="words.length === 0" class="empty-list">
              暂无单词，请手动添加或通过截图 OCR 提取
            </div>
          </div>

          <!-- 手动添加 -->
          <div class="add-word-row">
            <input
              v-model="newWord"
              type="text"
              class="word-input"
              placeholder="输入单词后回车添加..."
              @keydown.enter="addWord"
              :disabled="isImporting"
            />
            <button class="btn-sm btn-primary" @click="addWord" :disabled="!newWord.trim()">添加</button>
          </div>
        </div>

        <!-- 牌组选择 -->
        <div class="deck-section">
          <div class="section-header">
            <span class="section-title">牌组</span>
          </div>

          <div class="deck-options">
            <label class="radio-option">
              <input type="radio" :value="true" v-model="useCustomDeck" />
              <span>日期牌组</span>
            </label>
            <label class="radio-option">
              <input type="radio" :value="false" v-model="useCustomDeck" />
              <span>已有牌组</span>
            </label>
          </div>

          <div v-if="useCustomDeck" class="deck-input-row">
            <input
              v-model="customDeck"
              type="text"
              class="deck-input"
              :placeholder="todayDeck"
            />
          </div>
          <div v-else class="deck-select-row">
            <!-- 已选牌组显示 -->
            <div v-if="selectedDeck && !showDeckDropdown" class="selected-deck" @click="showDeckDropdown = true">
              <span class="selected-deck-name">{{ selectedDeck }}</span>
              <button class="selected-deck-clear" @click.stop="selectedDeck = ''" title="清除">✕</button>
            </div>
            <!-- 搜索输入框 -->
            <div v-else class="deck-search-wrapper">
              <input
                v-model="deckSearch"
                type="text"
                class="deck-search-input"
                placeholder="搜索牌组..."
                @focus="onDeckSearchFocus"
                @blur="onDeckSearchBlur"
                autofocus
              />
              <span class="deck-search-icon">🔍</span>
            </div>
            <!-- 下拉列表 -->
            <div v-if="showDeckDropdown" class="deck-dropdown">
              <div v-if="filteredDecks.length === 0" class="deck-dropdown-empty">
                无匹配牌组
              </div>
              <div
                v-for="d in filteredDecks"
                :key="d"
                class="deck-dropdown-item"
                @mousedown.prevent="selectDeck(d)"
              >
                <span class="deck-item-name">{{ deckDisplayName(d) }}</span>
                <span v-if="deckParentPath(d)" class="deck-item-path">{{ deckParentPath(d) }}</span>
              </div>
            </div>
          </div>
        </div>

        <!-- 导入按钮 & 进度 -->
        <div class="import-section">
          <div v-if="isImporting" class="progress-bar-container">
            <div class="progress-bar">
              <div class="progress-fill" :style="{ width: progressPercent + '%' }"></div>
            </div>
            <span class="progress-text">{{ importProgress }} / {{ importTotal }} ({{ progressPercent }}%)</span>
          </div>

          <div v-if="showResult && importResult" class="import-result">
            <span :class="importResult.success_count > 0 ? 'result-success' : 'result-warn'">
              导入完成：成功 {{ importResult.success_count }} / {{ importResult.total_count }}
            </span>
          </div>

          <div v-if="error" class="error-msg">{{ error }}</div>

          <div class="import-actions">
            <button class="btn btn-secondary" @click="closeWindow">关闭</button>
            <button
              class="btn btn-primary"
              :disabled="!canImport"
              @click="doImport"
            >
              <span v-if="importSubmitted" class="spinner-sm"></span>
              {{ importSubmitted ? `后台处理中... 导入 ${words.length} 个` : `导入 ${words.length} 个单词` }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.anki-card-app {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  font-family: var(--font-family);
  overflow: hidden;
}

/* 标题栏 */
.titlebar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 36px;
  padding: 0 12px;
  background: var(--color-bg-secondary);
  border-bottom: 1px solid var(--color-border-light);
  user-select: none;
  -webkit-user-select: none;
  flex-shrink: 0;
}

.titlebar-title {
  font-size: 13px;
  font-weight: 500;
  color: var(--color-text-secondary);
}

.titlebar-buttons {
  display: flex;
  align-items: center;
  gap: 2px;
}

.titlebar-btn {
  width: 36px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: 12px;
  transition: background 0.1s;
}

.titlebar-btn:hover {
  background: var(--color-bg-tertiary);
}

.titlebar-close:hover {
  background: var(--color-error) !important;
  color: white;
}

/* 主体布局 */
.main-content {
  flex: 1;
  display: flex;
  overflow: hidden;
}

/* 左侧预览 */
.preview-panel {
  width: 45%;
  min-width: 300px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--color-bg-secondary);
  border-right: 1px solid var(--color-border-light);
  overflow: hidden;
  padding: 16px;
}

.image-container {
  max-width: 100%;
  max-height: 100%;
  overflow: auto;
}

.preview-image {
  max-width: 100%;
  max-height: calc(100vh - 52px);
  object-fit: contain;
  border-radius: var(--radius-md);
}

.empty-preview {
  color: var(--color-text-tertiary);
  font-size: 14px;
}

/* 右侧控制面板 */
.control-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  padding: 16px;
  overflow-y: auto;
  gap: 16px;
}

/* 状态栏 */
.status-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-radius: var(--radius-sm);
  font-size: 13px;
}

.status-bar.loading {
  background: var(--color-info-light);
  color: var(--color-info);
}

.status-bar.error {
  background: var(--color-error-light);
  color: var(--color-error);
  flex-wrap: wrap;
}

.status-bar.success {
  background: var(--color-success-light);
  color: var(--color-success);
}

/* 区块标题 */
.section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}

.section-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--color-text-secondary);
}

/* 单词列表 */
.word-list-section {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.word-list {
  flex: 1;
  overflow-y: auto;
  border: 1px solid var(--color-border-light);
  border-radius: var(--radius-sm);
  padding: 4px;
  min-height: 120px;
  max-height: 300px;
}

.word-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px;
  border-radius: 4px;
  transition: background 0.1s;
}

.word-item:hover {
  background: var(--color-bg-tertiary);
}

.word-text {
  font-size: 14px;
  color: var(--color-text-primary);
  cursor: default;
}

.word-edit-input {
  flex: 1;
  font-size: 14px;
  padding: 2px 6px;
  border: 1px solid var(--color-accent);
  border-radius: 4px;
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  outline: none;
}

.word-remove {
  width: 20px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
  border-radius: 4px;
  font-size: 10px;
  opacity: 0;
  transition: opacity 0.1s;
}

.word-item:hover .word-remove {
  opacity: 1;
}

.word-remove:hover {
  background: var(--color-error-light);
  color: var(--color-error);
}

.empty-list {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  min-height: 80px;
  color: var(--color-text-tertiary);
  font-size: 13px;
}

.add-word-row {
  display: flex;
  gap: 8px;
  margin-top: 8px;
}

.word-input {
  flex: 1;
  padding: 6px 10px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  font-size: 13px;
  outline: none;
}

.word-input:focus {
  border-color: var(--color-border-focus);
}

/* 牌组选择 */
.deck-section {
  flex-shrink: 0;
}

.deck-options {
  display: flex;
  gap: 16px;
  margin-bottom: 8px;
}

.radio-option {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--color-text-secondary);
  cursor: pointer;
}

.deck-input-row, .deck-select-row {
  width: 100%;
  position: relative;
}

.deck-input {
  width: 100%;
  padding: 6px 10px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  font-size: 13px;
  outline: none;
}

.deck-input:focus {
  border-color: var(--color-border-focus);
}

/* 已选牌组 */
.selected-deck {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-input-bg);
  cursor: pointer;
  font-size: 13px;
  transition: border-color 0.15s;
}

.selected-deck:hover {
  border-color: var(--color-border-focus);
}

.selected-deck-name {
  color: var(--color-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

.selected-deck-clear {
  width: 18px;
  height: 18px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
  border-radius: 50%;
  font-size: 10px;
  flex-shrink: 0;
  margin-left: 6px;
}

.selected-deck-clear:hover {
  background: var(--color-error-light);
  color: var(--color-error);
}

/* 搜索输入 */
.deck-search-wrapper {
  position: relative;
}

.deck-search-input {
  width: 100%;
  padding: 6px 10px 6px 28px;
  border: 1px solid var(--color-border-focus);
  border-radius: var(--radius-sm);
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  font-size: 13px;
  outline: none;
  box-sizing: border-box;
}

.deck-search-icon {
  position: absolute;
  left: 8px;
  top: 50%;
  transform: translateY(-50%);
  font-size: 12px;
  pointer-events: none;
  opacity: 0.5;
}

/* 下拉列表 */
.deck-dropdown {
  position: absolute;
  bottom: 100%;
  left: 0;
  right: 0;
  max-height: 280px;
  overflow-y: auto;
  background: var(--color-bg-primary);
  border: 1px solid var(--color-border);
  border-bottom: none;
  border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  box-shadow: 0 -4px 12px rgba(0, 0, 0, 0.12);
  z-index: 100;
}

.deck-dropdown-empty {
  padding: 10px 12px;
  font-size: 12px;
  color: var(--color-text-tertiary);
  text-align: center;
}

.deck-dropdown-item {
  padding: 6px 10px;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 1px;
  transition: background 0.1s;
}

.deck-dropdown-item:hover {
  background: var(--color-accent);
  color: white;
}

.deck-dropdown-item:hover .deck-item-path {
  color: rgba(255, 255, 255, 0.7);
}

.deck-item-name {
  font-size: 13px;
  font-weight: 500;
}

.deck-item-path {
  font-size: 11px;
  color: var(--color-text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.deck-dropdown::-webkit-scrollbar {
  width: 6px;
}

.deck-dropdown::-webkit-scrollbar-track {
  background: transparent;
}

.deck-dropdown::-webkit-scrollbar-thumb {
  background: var(--scrollbar-thumb);
  border-radius: 3px;
}

/* 导入区 */
.import-section {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.progress-bar-container {
  display: flex;
  align-items: center;
  gap: 12px;
}

.progress-bar {
  flex: 1;
  height: 6px;
  background: var(--color-bg-tertiary);
  border-radius: 3px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: var(--color-accent);
  transition: width 0.3s;
  border-radius: 3px;
}

.progress-text {
  font-size: 12px;
  color: var(--color-text-tertiary);
  white-space: nowrap;
}

.import-result {
  padding: 8px 12px;
  border-radius: var(--radius-sm);
  font-size: 13px;
}

.result-success {
  color: var(--color-success);
}

.result-warn {
  color: var(--color-warning);
}

.error-msg {
  padding: 8px 12px;
  border-radius: var(--radius-sm);
  background: var(--color-error-light);
  color: var(--color-error);
  font-size: 12px;
}

.import-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

/* 按钮 */
.btn {
  padding: 8px 20px;
  border: none;
  border-radius: var(--radius-sm);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 6px;
  transition: opacity 0.15s;
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-primary {
  background: var(--color-accent);
  color: white;
}

.btn-primary:hover:not(:disabled) {
  opacity: 0.9;
}

.btn-secondary {
  background: var(--color-bg-tertiary);
  color: var(--color-text-primary);
}

.btn-secondary:hover:not(:disabled) {
  opacity: 0.8;
}

.btn-sm {
  padding: 4px 10px;
  border: none;
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  background: var(--color-bg-tertiary);
  color: var(--color-text-secondary);
}

.btn-sm:hover {
  opacity: 0.8;
}

.btn-sm.btn-primary {
  background: var(--color-accent);
  color: white;
}

.btn-sm.btn-danger {
  background: var(--color-error-light);
  color: var(--color-error);
}

/* 加载动画 */
.spinner {
  width: 14px;
  height: 14px;
  border: 2px solid currentColor;
  border-top-color: transparent;
  border-radius: 50%;
  animation: spin 0.6s linear infinite;
  display: inline-block;
}

.spinner-sm {
  width: 12px;
  height: 12px;
  border: 2px solid rgba(255, 255, 255, 0.3);
  border-top-color: white;
  border-radius: 50%;
  animation: spin 0.6s linear infinite;
  display: inline-block;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* 滚动条 */
.word-list::-webkit-scrollbar {
  width: 6px;
}

.word-list::-webkit-scrollbar-track {
  background: transparent;
}

.word-list::-webkit-scrollbar-thumb {
  background: var(--scrollbar-thumb);
  border-radius: 3px;
}

.control-panel::-webkit-scrollbar {
  width: 6px;
}

.control-panel::-webkit-scrollbar-track {
  background: transparent;
}

.control-panel::-webkit-scrollbar-thumb {
  background: var(--scrollbar-thumb);
  border-radius: 3px;
}
</style>
