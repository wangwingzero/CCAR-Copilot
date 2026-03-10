/**
 * Vue I18n 类型声明
 *
 * 为 Vue 3 组件添加 $t, $tc, $n, $d 全局属性的类型支持
 *
 * @validates Requirements 17.6
 */

import type { DefineLocaleMessage } from 'vue-i18n'

// DefineLocaleMessage 用于扩展 vue-i18n 类型系统
// eslint-disable-next-line @typescript-eslint/no-unused-vars
type _LocaleMessage = DefineLocaleMessage

declare module '@vue/runtime-core' {
  interface ComponentCustomProperties {
    $t: (key: string, ...args: unknown[]) => string
    $tc: (key: string, choice?: number, ...args: unknown[]) => string
    $n: (value: number, ...args: unknown[]) => string
    $d: (value: Date, ...args: unknown[]) => string
  }
}

export {}
