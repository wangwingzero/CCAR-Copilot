/**
 * Anki 单词卡制作窗口入口
 *
 * 独立窗口，用于从截图中提取英文单词并批量导入 Anki。
 * 功能：
 * - 显示截图预览
 * - OCR 文字提取英文单词
 * - 单词列表编辑（添加、删除）
 * - 牌组选择（日期牌组 / 自定义）
 * - 批量导入到 Anki（查词典、下载发音、获取配图）
 */

import { createApp } from 'vue'
import { createPinia } from 'pinia'
import AnkiCardApp from './AnkiCardApp.vue'
import './styles/theme.css'
import { initializeTheme } from './composables/useTheme'

// 在 Vue 挂载前初始化主题，防止白屏闪烁
initializeTheme()

const app = createApp(AnkiCardApp)
const pinia = createPinia()

app.use(pinia)
app.mount('#app')
