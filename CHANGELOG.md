# Changelog

本文件记录虎哥截图 Tauri 版本的所有重要变更。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

---

## [Unreleased]

### 新增

#### 规章管理系统

- **扫描+OCR 一键操作** — 合并本地扫描与 OCR 为单一命令
  - `regulation_scan_local_dir` 新增 `auto_ocr` 参数（默认 true），扫描完自动执行 OCR
  - 进度条支持扫描阶段和 OCR 阶段的连续显示
  - 移除独立的"OCR扫描版"按钮，简化用户操作
  - 新增 `regulation_retry_failed_ocr` 命令，支持重试失败的 OCR 文件
  - 扫描结果显示直接索引数、OCR 索引数，失败时显示重试按钮

### 性能优化

- **启动性能优化** — 解决应用启动时"未响应"黑屏问题
  - 将 `init_file_search_state` 缓存加载从主线程移到后台线程
  - 主线程 setup 从数秒优化到 0.05 秒完成
  - 新增 `app:ready` 事件，后台初始化完成后通知前端
  - 前端添加启动加载屏（splash screen），带动画和超时兜底

### 修复

- **截图引擎文档修正** — 统一为 WGC 优先、DXGI 仅用于录屏的准确描述
- **预截图缓存竞态修复** — `take_pre_capture_cache` 改为 `clone()`，避免 `capture_region` 找不到缓存

### 重构

- **规章状态管理重构** — 消除 store 和 composable 之间的职责混乱
  - store 添加 `startSyncCompare`/`finishSyncCompare` 封装方法
  - 组件统一通过 composable 访问 store 状态，移除直接 store 访问
  - 消除 `scanError`/`error` 重复状态复制

### 新增（之前）

#### 截图引擎

- **WGC (Windows Graphics Capture) 截图引擎** — 全新的截图捕获方案
  - 通过 HMONITOR 精确匹配显示器，彻底解决多显示器 ID 不一致问题
  - 支持 D3D11 设备缓存，重复截图性能极高
  - 截图策略升级为三级回退：WGC → DXGI → GDI (screenshots-rs)
  - 新增 `wgc_capture.rs` 模块（~310 行），支持 Windows 10 1903+

#### 规章管理系统

- **本地目录扫描** — 批量导入本地 PDF 规章文件
  - 支持递归扫描子目录，自动识别 PDF 文件
  - SHA256 文件哈希去重，避免重复入库
  - 文件名智能解析：自动提取文号（AC-xxx、CCAR-xxx、IB-xxx 等）和文档类型
  - pdf-extract 自动提取可选择文本，不可选择的标记为待 OCR
  - 实时进度事件 `regulation:scan-progress`，前端展示扫描进度条
  - 新增 `regulation_scan_local_dir` Tauri 命令

- **纯 Rust PDF OCR** — 替代 Python sidecar OCR 方案
  - 使用 pdfium-render 渲染 PDF 页面为图片，调用 PP-OCRv4 + OpenVINO 进行文字识别
  - 新增 `pdf_ocr.rs` 模块（~290 行），零外部 Python 依赖
  - 新增 `regulation_ocr_pending` / `regulation_ocr_update` / `regulation_get_ocr_queue` 命令
  - 捆绑 `pdfium.dll`（~5.5MB）作为 Tauri 资源

- **官网同步对比** — 与 CAAC 官网规章列表进行全量对比
  - Python sidecar 新增 `fetch_all` 方法，分页全量爬取规章列表
  - Rust 端 `regulation_sync_compare` 命令对比本地数据库差异
  - 展示新增规章、有效性变化、仅本地存在的文件

- **数据库统计仪表板** — 前端新增可视化统计
  - 显示总文件数、已索引、待处理、失败数量
  - 彩色进度条直观展示各状态占比（绿/黄/红）

- **搜索偏好持久化** — 自动记忆用户搜索设置
  - 搜索模式（在线/本地/混合）持久化到 localStorage
  - 筛选条件（文档类型、有效性、日期范围、关键词）自动保存和恢复

### 性能优化

- **截图复制/保存全面优化** — 消除巨型 JSON 序列化瓶颈
  - 新增 `save_screenshot_with_history_from_file` 命令，通过文件路径传递图像数据
  - 新增 `copy_file_to_clipboard` 命令，后端直接从磁盘读取文件写入剪贴板
  - 复制操作改为非阻塞：窗口立即关闭（~50ms），剪贴板写入在 Rust 后台完成
  - `handleCopy` 添加防重入保护（`isCopyInProgress`），防止双击触发多次调用
- **多场景统一优化** — OCR、钉图、Anki 等功能统一使用 `writeFile` 二进制 IPC
  - 替代 `Array.from(pngData)` + JSON 序列化的低效方案
  - 减少前后端数据传输量，尤其对大图像（>5MB）提升显著

### Bug 修复

- **DXGI 显示器匹配修复** — 改用屏幕坐标 (x, y, width, height) 匹配 DXGI 输出
  - 解决 screenshots-rs 原生 ID 与 DXGI 枚举索引不一致导致的截图黑屏/错屏问题
  - 录屏模块同步更新为坐标匹配方式
- **pdf-extract panic 防护** — 添加 `catch_unwind` 包裹
  - 修复不支持的 CID 字体编码（非 Identity-H）导致整个进程 crash
- **热键注册空值保护** — 跳过空字符串热键配置
  - 修复用户清空某个热键后保存导致启动异常
- **显示器索引回退** — `capture_screen` 增加按索引查找
  - 兼容 overlay 传入索引而非原生显示器 ID 的边界情况

### 依赖变更

- 新增 `pdfium-render = "0.8.37"`（PDF 渲染，用于规章 OCR）
- 新增 Windows API features：`Graphics_Capture`、`Graphics_DirectX`、`Win32_System_WinRT_Graphics_Capture` 等（WGC 截图引擎）
- 捆绑 `pdfium.dll`（Google PDFium 原生库，含许可证文件）

---

## [0.1.0] - 2026-01-24

### 新增

#### Rust 核心 (src-tauri)

- **截图引擎**
  - 使用 `screenshots-rs` 实现屏幕捕获
  - 支持多显示器截图
  - 支持高 DPI 场景（返回物理像素尺寸和 DPR）
  - 截图保存为临时文件，通过 `asset://` 协议访问

- **窗口检测**
  - 使用 Windows API 实现窗口边界检测
  - 支持获取窗口标题、类名、句柄
  - 支持指定坐标点的窗口查找

- **全局热键**
  - 使用 `tauri-plugin-global-shortcut` 注册热键
  - 默认截图热键 `Alt+A`
  - 支持热键配置持久化
  - 热键冲突检测和通知

- **窗口管理**
  - 覆盖窗口（截图选区）：全屏透明、置顶、捕获鼠标事件
  - 钉图窗口：支持调整大小、移动、透明度调节
  - 显示器信息获取：位置、尺寸、DPR、主显示器标识

- **Sidecar 管理器**
  - Python Sidecar 进程启动和停止
  - stdin/stdout JSON 通信协议
  - 请求/响应 ID 匹配
  - 崩溃自动重启机制（规划中）

- **数据库**
  - SQLite 历史记录表结构
  - 设置存储模块

- **设备管理**（独立模块，未集成）
  - 设备指纹生成（SMBIOS UUID + MAC + 磁盘序列号）
  - SHA-256 哈希

- **单实例锁**（独立模块，未集成）
  - Windows Mutex 实现
  - 重复启动检测

- **系统托盘**
  - 托盘图标配置
  - 托盘菜单

#### Vue 前端 (src)

- **状态管理 (Pinia)**
  - `screenshot.ts` - 截图状态
  - `annotation.ts` - 标注状态
  - `history.ts` - 历史记录状态
  - `settings.ts` - 设置状态
  - `sidecar.ts` - Sidecar 服务状态

- **截图组件**
  - `ScreenshotOverlay.vue` - 截图选区覆盖层

- **标注系统**
  - `AnnotationCanvas.vue` - 标注画布核心
  - `Toolbar.vue` - 工具栏组件
  - Command Pattern 实现 Undo/Redo

- **形状标注工具**
  - 矩形工具
  - 椭圆工具
  - 箭头工具
  - 直线工具
  - 支持颜色、线宽配置

- **文字标注工具**
  - 文字输入和编辑
  - 支持字体、颜色、大小配置

- **隐私工具**
  - 马赛克效果（像素化）
  - 高斯模糊效果

- **TypeScript 类型**
  - `screenshot.ts` - 截图相关类型
  - `annotation.ts` - 标注相关类型
  - `sidecar.ts` - Sidecar 协议类型
  - `config.ts` - 配置类型
  - `history.ts` - 历史记录类型

#### 文档

- `README.md` - 项目说明文档
- `CHANGELOG.md` - 版本变更记录

### 技术栈

#### Rust 依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| tauri | 2.x | 应用框架 |
| screenshots | 0.8 | 屏幕捕获 |
| tauri-plugin-global-shortcut | 2.x | 全局热键 |
| tauri-plugin-fs | 2.x | 文件系统 |
| tauri-plugin-shell | 2.x | Shell 命令 |
| tauri-plugin-clipboard-manager | 2.x | 剪贴板 |
| tauri-plugin-dialog | 2.x | 对话框 |
| tokio | 1.x | 异步运行时 |
| rusqlite | 0.32 | SQLite |
| windows | 0.58 | Windows API |
| tracing | 0.1 | 日志 |
| thiserror | 2.x | 错误处理 |
| uuid | 1.x | UUID 生成 |
| proptest | 1.x | 属性测试 |

#### Vue 依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| vue | 3.5 | UI 框架 |
| pinia | 3.x | 状态管理 |
| @tauri-apps/api | 2.x | Tauri IPC |
| typescript | 5.6 | 类型系统 |
| vite | 6.x | 构建工具 |

---

## 版本对比

| 版本 | 状态 | 说明 |
|------|------|------|
| Python v2.9.1 | 生产版本 | 当前稳定版本，功能完整 |
| Tauri v0.1.0 | 开发中 | 核心功能已实现，服务集成进行中 |

---

## 链接

- [Python 版本 README](../README.md)
- [设计文档](../.kiro/specs/tauri-rust-python-rewrite/design.md)
- [任务清单](../.kiro/specs/tauri-rust-python-rewrite/tasks.md)
