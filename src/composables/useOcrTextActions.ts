/**
 * OCR 文本操作共享 composable
 *
 * 提供 OCR 文本处理的统一逻辑，被 OCR 结果弹窗和工作台面板共享使用。
 * 包括：
 * - 文本格式化（合并为单行、智能分段、移除空格、标点转换）
 * - 恢复原文
 * - 复制到剪贴板
 * - 翻译
 * - 本地 OCR 重新识别
 * - Markdown 转换
 *
 * 设计原则：
 * - 纯格式化函数可独立使用（applyFormat / applyMarkdownConversion）
 * - composable 接受外部 Ref，适配不同状态管理模式
 * - 一处修改，两处（OCR 弹窗 + 工作台）同步生效
 */

import { type Ref, computed } from 'vue'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { invoke } from '@tauri-apps/api/core'
import { useSidecarStore } from '@/stores/sidecar'

// ============================================
// Types
// ============================================

/** 文本格式化类型（与 OcrToolbar 保持一致） */
export type FormatType =
  | 'merge-lines'
  | 'smart-paragraphs'
  | 'remove-spaces'
  | 'punct-to-en'
  | 'punct-to-cn'
  | 'clean-symbols'

/** composable 配置选项 */
export interface OcrTextActionsOptions {
  /** 获取当前图片路径（用于本地 OCR 重新识别） */
  getImagePath?: () => string | null
  /** 外部加载状态 Ref（可选，composable 会在操作期间设置） */
  isLoading?: Ref<boolean>
}

// ============================================
// 纯函数：文本格式化
// ============================================

// ---- 符号噪声过滤常量 ----
// 将 clean-symbols 使用的正则按类别拆分为命名常量，方便维护和扩展。
//
// 注意：Rust 端 OCR 引擎（ocr/engine.rs 的 is_symbol_noise）已在识别阶段过滤
// 1-2 字符的纯符号 region（如 `>`、`{}`）。前端此处是**文本级别的补充清理**，
// 针对多字符行首符号残留和纯符号行，两层过滤互补、不冲突。

/** 箭头类符号（文件夹展开箭头、导航箭头等） */
const ARROWS = '>›»«‹<►▶▸▼▾▲△▻▹'
/** 勾选/叉号类符号（复选框图标） */
const CHECK_MARKS = '☐☑☒✓✗✘×'
/** 几何图形符号（文件/文件夹图标） */
const GEOMETRIC = '⊕⊗⊙■□▪▫◆◇◈'
/** 装饰/特殊符号（星号、项目符号、警告图标等） */
const DECORATIVE = '⬤⬢⬡※☆★⚠⚡♦♣♠♥'
/** 系统/键盘符号（macOS 按键图标等） */
const SYSTEM = '⌂⌘⌥⌃⌤⎋⏎'

/** 所有 UI 噪声符号集合 */
const ALL_NOISE_SYMBOLS = ARROWS + CHECK_MARKS + GEOMETRIC + DECORATIVE + SYSTEM

/**
 * 行首 UI 噪声符号正则：匹配单个噪声符号或成对括号符号（`{}`、`[]`、`()`、`<>`）
 * 捕获组 $1 保留行首缩进
 */
const RE_LINE_START_NOISE = new RegExp(
  `^([ \\t]*)(?:[${ALL_NOISE_SYMBOLS.replace(/[-\\^$*+?.()|[\]{}]/g, '\\$&')}]|\\{\\}|\\[\\]|\\(\\)|<>)[ \\t]*`,
)

/** 行首连续 > 符号（如 `>>` 或 `>>>` + 空格） */
const RE_LINE_START_ARROWS = /^([ \t]*)>{1,3}[ \t]+/

/** 行首孤立的单个标点符号（后面跟空格 + 实际文本内容） */
const RE_LINE_START_LONE_PUNCT =
  /^([ \t]*)[·•|^~`#$%&@!?][ \t]+(?=[\w\u4e00-\u9fff\u3400-\u4dbf.])/

/** 包含有效文本内容（字母、数字或汉字）的行 */
const RE_HAS_REAL_CONTENT = /[\da-zA-Z\u4e00-\u9fff\u3400-\u4dbf\uF900-\uFAFF]/

/**
 * 对文本应用格式化（纯函数，无副作用）
 *
 * 被 composable 和 workbenchStore 共享调用。
 *
 * @param text 原始文本
 * @param type 格式化类型
 * @returns 格式化后的文本
 */
export function applyFormat(text: string, type: FormatType): string {
  let formatted = text

  switch (type) {
    case 'merge-lines':
      // 合并为单行：移除所有换行符
      formatted = formatted.replace(/\r?\n/g, '')
      break

    case 'smart-paragraphs':
      // 智能分段：连续换行保留，单个换行替换为空格
      formatted = formatted
        .replace(/\r\n/g, '\n')
        .replace(/([^\n])\n([^\n])/g, '$1 $2')
      break

    case 'remove-spaces':
      // 移除多余空格
      formatted = formatted.replace(/[ \t]+/g, ' ').trim()
      break

    case 'punct-to-en':
      // 中文标点转英文
      formatted = formatted
        .replace(/，/g, ',')
        .replace(/。/g, '.')
        .replace(/！/g, '!')
        .replace(/？/g, '?')
        .replace(/：/g, ':')
        .replace(/；/g, ';')
        .replace(/（/g, '(')
        .replace(/）/g, ')')
        .replace(/【/g, '[')
        .replace(/】/g, ']')
        .replace(/\u201c/g, '"')
        .replace(/\u201d/g, '"')
        .replace(/\u2018/g, "'")
        .replace(/\u2019/g, "'")
      break

    case 'punct-to-cn':
      // 英文标点转中文
      formatted = formatted
        .replace(/,/g, '，')
        .replace(/\./g, '。')
        .replace(/!/g, '！')
        .replace(/\?/g, '？')
        .replace(/:/g, '：')
        .replace(/;/g, '；')
        .replace(/\(/g, '（')
        .replace(/\)/g, '）')
        .replace(/\[/g, '【')
        .replace(/\]/g, '】')
      break

    case 'clean-symbols':
      // 清理 OCR 符号噪声（文本级别补充清理，与 Rust 端引擎级过滤互补）
      // 常见于截图中 UI 图标（文件夹箭头、文件类型图标、状态图标等）被误识别为文字字符
      formatted = formatted
        .split('\n')
        .map((line) => {
          let cleaned = line

          // 1. 移除行首的明确噪声符号（不可能是正常文本内容的）
          cleaned = cleaned.replace(RE_LINE_START_NOISE, '$1')

          // 2. 移除行首的连续 > 符号（如 >> 或 >>> ）
          cleaned = cleaned.replace(RE_LINE_START_ARROWS, '$1')

          // 3. 移除行首孤立的单个符号字符（后面跟空格+实际文本内容）
          //    例如 "! releaseyml" 中的 "!"、"· 文本" 中的 "·"
          cleaned = cleaned.replace(RE_LINE_START_LONE_PUNCT, '$1')

          return cleaned
        })
        .filter((line) => {
          // 4. 过滤掉纯符号行（不含任何字母、数字或汉字）
          const stripped = line.trim()
          if (stripped.length === 0) return true // 保留空行
          return RE_HAS_REAL_CONTENT.test(stripped)
        })
        .join('\n')

      // 5. 清理因移除符号产生的多余空行
      formatted = formatted.replace(/\n{3,}/g, '\n\n').trim()
      break
  }

  return formatted
}

/**
 * 对文本应用 Markdown 转换（纯函数，无副作用）
 *
 * 增强的转换规则：
 * 1. 识别多级标题（一/二/三级、中文数字、罗马数字等）
 * 2. 识别有序和无序列表
 * 3. 识别引用块
 * 4. 智能段落分隔
 * 5. 识别加粗/强调关键词
 * 6. 识别简单表格结构
 *
 * @param text 原始文本
 * @returns Markdown 格式化后的文本
 */
export function applyMarkdownConversion(text: string): string {
  const lines = text.split('\n')
  const result: string[] = []

  for (let i = 0; i < lines.length; i++) {
    let line = lines[i]
    const trimmed = line.trim()

    // 跳过已有 Markdown 格式的行（避免重复转换）
    if (/^#{1,6}\s/.test(trimmed)) {
      result.push(line)
      continue
    }

    // 1. 识别一级标题：
    //    - 中文书名号标题如：《关于XX的通知》（独立成行且较短）
    //    - "第X章"、"第X部分" 等
    if (
      /^第[一二三四五六七八九十百千\d]+[章篇部节回]\s*.+/.test(trimmed) ||
      /^[（(][一二三四五六七八九十]+[）)]\s*.+/.test(trimmed) && trimmed.length <= 40
    ) {
      result.push(`# ${trimmed}`)
      continue
    }

    // 2. 识别二级标题：
    //    - "一、"、"二、" 等中文数字编号
    //    - "1."、"2." 等阿拉伯数字编号（行长度较短，像标题）
    if (/^[一二三四五六七八九十]+[、.．]\s*.+/.test(trimmed) && trimmed.length <= 60) {
      result.push(`## ${trimmed}`)
      continue
    }

    if (/^\d+[.、．]\s*.+/.test(trimmed) && trimmed.length <= 50) {
      // 检测是否更像标题而非段落内容（无句号/逗号结尾）
      if (!/[。，；！？,.;!?]$/.test(trimmed)) {
        result.push(`## ${trimmed}`)
        continue
      }
    }

    // 3. 识别三级标题：
    //    - "(1)"、"（1）"、"1)" 等子编号
    //    - "1.1"、"1.2" 等层级编号
    if (
      (/^[（(]\d+[）)]\s*.+/.test(trimmed) || /^\d+\)\s*.+/.test(trimmed)) &&
      trimmed.length <= 50 &&
      !/[。，；！？,.;!?]$/.test(trimmed)
    ) {
      result.push(`### ${trimmed}`)
      continue
    }

    if (/^\d+\.\d+[.、．]?\s*.+/.test(trimmed) && trimmed.length <= 50) {
      result.push(`### ${trimmed}`)
      continue
    }

    // 4. 识别无序列表项
    if (/^[·•●○◆◇▪▸►➤➢→]\s*/.test(trimmed)) {
      line = trimmed.replace(/^[·•●○◆◇▪▸►➤➢→]\s*/, '- ')
      result.push(line)
      continue
    }

    // 已有的列表标记统一
    if (/^[-*]\s+/.test(trimmed)) {
      result.push(trimmed.replace(/^[-*]\s+/, '- '))
      continue
    }

    // 5. 识别引用块（缩进文本或以"注："、"备注："开头）
    if (/^[注备说][:：]/.test(trimmed) || /^注意[:：]/.test(trimmed) || /^提示[:：]/.test(trimmed)) {
      result.push(`> ${trimmed}`)
      continue
    }

    // 6. 识别关键词加粗（冒号前的标签词）
    if (/^[\u4e00-\u9fff]{2,6}[:：]\s*.+/.test(trimmed)) {
      line = trimmed.replace(/^([\u4e00-\u9fff]{2,6})([:：])/, '**$1**$2')
      result.push(line)
      continue
    }

    // 7. 默认保留原文
    result.push(line)
  }

  let markdown = result.join('\n')

  // 8. 在标题前添加空行（如果前面不是空行）
  markdown = markdown.replace(/([^\n])\n(#{1,3}\s)/g, '$1\n\n$2')

  // 9. 清理多余空行（3个以上合并为2个）
  markdown = markdown.replace(/\n{3,}/g, '\n\n')

  return markdown
}

// ============================================
// Composable
// ============================================

/**
 * OCR 文本操作 composable
 *
 * 使用方式：
 * ```ts
 * const textRef = ref('')
 * const originalRef = ref('')
 * const { formatText, copyText, translateText, ... } = useOcrTextActions(textRef, originalRef)
 * ```
 *
 * @param textRef 当前文本的 Ref
 * @param originalTextRef 原始文本的 Ref（用于恢复原文）
 * @param options 配置选项
 */
export function useOcrTextActions(
  textRef: Ref<string>,
  originalTextRef: Ref<string>,
  options?: OcrTextActionsOptions
) {
  const sidecarStore = useSidecarStore()
  const externalLoading = options?.isLoading

  // ============================================
  // Computed
  // ============================================

  /** 是否有内容 */
  const hasContent = computed(() => textRef.value.length > 0)

  /** 文本是否已修改（与原文不同） */
  const hasChanges = computed(() => textRef.value !== originalTextRef.value)

  /** 字符数 */
  const charCount = computed(() => textRef.value.length)

  // ============================================
  // 文本格式化
  // ============================================

  /**
   * 格式化文本
   * @param type 格式化类型
   */
  function formatText(type: FormatType): void {
    if (!textRef.value) return
    textRef.value = applyFormat(textRef.value, type)
  }

  /**
   * 恢复原始文本
   */
  function restoreOriginal(): void {
    textRef.value = originalTextRef.value
  }

  /**
   * 转换为 Markdown 格式
   */
  function convertToMarkdown(): void {
    if (!textRef.value) return
    textRef.value = applyMarkdownConversion(textRef.value)
  }

  // ============================================
  // 剪贴板
  // ============================================

  /**
   * 复制文本到剪贴板
   * @returns 是否成功
   */
  async function copyText(): Promise<boolean> {
    if (!textRef.value) return false

    try {
      await writeText(textRef.value)
      return true
    } catch (error) {
      console.error('[useOcrTextActions] 复制失败:', error)
      return false
    }
  }

  // ============================================
  // 翻译
  // ============================================

  /** 直接翻译结果类型（与 Rust DirectTranslationResult 对应） */
  interface DirectTranslateResult {
    translatedText: string
    sourceLang: string
    targetLang: string
    provider: string
  }

  /**
   * 翻译文本（智能语言检测，不依赖 Sidecar）
   *
   * 参考 Python 版本的 _do_smart_translate：
   * - 检测文本是否包含中文
   * - 中文→翻译为英语，非中文→翻译为中文
   *
   * 优先使用 Rust 原生直接翻译（免费 MyMemory API），
   * 无需 Python Sidecar 即可工作。
   *
   * @param targetLang 目标语言（可选，不提供时自动检测）
   * @throws Error 翻译服务返回空结果时抛出
   */
  async function translateText(targetLang?: string): Promise<void> {
    if (!textRef.value) return

    try {
      if (externalLoading) externalLoading.value = true

      // 直接调用 Rust 原生翻译命令（不依赖 Sidecar）
      const result = await invoke<DirectTranslateResult>('translate_text_direct', {
        text: textRef.value,
        targetLang: targetLang || null,
      })

      if (result.translatedText) {
        textRef.value = result.translatedText
      } else {
        throw new Error('翻译服务返回空结果')
      }
    } finally {
      if (externalLoading) externalLoading.value = false
    }
  }

  // ============================================
  // 本地 OCR
  // ============================================

  /**
   * 使用本地 OCR 重新识别
   * @returns OCR 识别结果文本
   */
  async function performLocalOcr(): Promise<{ text: string; confidence: number; elapsedTime: number }> {
    const imagePath = options?.getImagePath?.()
    if (!imagePath) {
      throw new Error('没有可识别的图片')
    }

    try {
      if (externalLoading) externalLoading.value = true

      const startTime = Date.now()
      const result = await sidecarStore.callOcr(imagePath)
      const elapsedTime = Date.now() - startTime

      // 提取文本
      const text = result.boxes?.map((box) => box.text).join('\n') ?? ''

      // 计算平均置信度
      const avgConfidence =
        result.boxes && result.boxes.length > 0
          ? result.boxes.reduce((sum, box) => sum + (box.confidence ?? 0), 0) /
            result.boxes.length
          : 0

      // 更新文本
      textRef.value = text
      originalTextRef.value = text

      return {
        text,
        confidence: Math.round(avgConfidence * 100),
        elapsedTime,
      }
    } finally {
      if (externalLoading) externalLoading.value = false
    }
  }

  return {
    // Computed
    hasContent,
    hasChanges,
    charCount,

    // 文本操作
    formatText,
    restoreOriginal,
    convertToMarkdown,
    copyText,
    translateText,
    performLocalOcr,
  }
}
