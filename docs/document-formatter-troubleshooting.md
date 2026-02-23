# 公文格式化（Word排版）常见问题与解决方案

## 问题：未检测到打开的 Word/WPS 文档

### 症状

打开虎哥截图的「Word排版」功能时，显示"未检测到打开的 Word/WPS 文档"，即使 Word 或 WPS 明明已经打开了文档。

### 根本原因：权限隔离（管理员 vs 普通用户）

Windows 有一个安全机制叫 **UIPI（User Interface Privilege Isolation）**：
- **以管理员身份运行的程序** 无法通过 COM 连接到 **以普通用户身份运行的程序**
- 反之亦然

常见场景：
1. **Cursor IDE 以管理员身份运行** → `npm run tauri dev` 启动的虎哥截图也是管理员
2. **Word/WPS 以普通用户运行**（正常启动）
3. 两者权限不匹配 → COM 调用返回 `MK_E_UNAVAILABLE` 错误

### 技术细节

虎哥截图通过以下方式检测打开的文档：

1. **方法 1：Win32 API 窗口枚举**（`EnumWindows`）
   - 不依赖 COM，不受权限限制
   - 可以检测到文档名称
   - **但无法获取文档的完整文件路径**
   - **无法操作文档内容**（只能看不能改）

2. **方法 2：COM 自动化**（`GetActiveObject` / Running Object Table）
   - 通过 `Word.Application` 或 `KWPS.Application` COM 接口连接
   - 可以获取文档完整路径
   - **可以直接操作打开的文档**（修改格式、保存等）
   - **受权限隔离限制**：必须与 Word/WPS 在同一权限级别

### 解决方案

#### 方案 1：以相同权限运行（推荐）

**开发环境：**
- 从一个**非管理员终端**运行 `npm run tauri dev`
- 这样虎哥截图与 Word/WPS 都以普通用户身份运行
- COM 连接正常工作

**生产环境：**
- 虎哥截图默认以普通用户身份运行（不需要管理员权限）
- Word/WPS 也通常以普通用户身份运行
- 一般不会遇到此问题

**如果需要以管理员运行虎哥截图：**
- 同时以管理员身份运行 Word/WPS
- 右键 Word/WPS → 「以管理员身份运行」

#### 方案 2：使用文件路径格式化

如果无法解决权限问题，可以使用替代方案：
1. 在虎哥截图中看到文档名称（通过窗口枚举检测到）
2. 点击「浏览」按钮手动选择文件
3. 使用 python-docx 进行文件级格式化（不需要 COM）
4. 格式化后的文档会保存为新文件（原文件名 + `_formatted` 后缀）

#### 方案 3：关闭文档后格式化

1. 保存并关闭 Word/WPS 中的文档
2. 在虎哥截图中使用「浏览」选择文档
3. 格式化后重新打开

## 技术架构

### 文档检测流程

```
用户点击「Word排版」
    │
    ├─ Step 1: Win32 API 窗口枚举（纯 Rust）
    │   ├─ EnumWindows 枚举所有可见窗口
    │   ├─ 通过进程名 (wps.exe / WINWORD.EXE) 过滤
    │   ├─ 从窗口标题提取文档名
    │   └─ 返回文档列表（无完整路径）
    │
    ├─ Step 2: COM 补充信息（Python Sidecar）
    │   ├─ Running Object Table (ROT) 枚举
    │   ├─ GetActiveObject 连接
    │   └─ 获取完整文件路径
    │
    └─ 合并结果显示给用户
```

### 格式化流程

```
用户选择文档 → 点击「开始格式化」
    │
    ├─ 有完整路径 → COM 方式格式化
    │   └─ 通过 Word/WPS COM 接口直接修改打开的文档
    │
    └─ 无完整路径 → 文件方式格式化
        ├─ 弹出文件选择对话框
        ├─ 用户选择文件
        └─ python-docx 格式化并保存副本
```

### WPS COM ProgID 参考

| ProgID | 说明 |
|---|---|
| `Word.Application` | Microsoft Word / WPS 兼容模式 |
| `KWPS.Application` | WPS Writer（经典 ProgID） |
| `wps.Application` | WPS Writer（现代版本） |
| `KET.Application` | WPS Spreadsheet |

### COM 错误代码参考

| 错误代码 | 名称 | 含义 |
|---|---|---|
| `0x800401E3` | `MK_E_UNAVAILABLE` | COM 对象已注册但无法访问（权限不匹配） |
| `0x800401F3` | `CO_E_CLASSSTRING` | ProgID 未注册 |
| `0x80040154` | `REGDB_E_CLASSNOTREG` | COM 类未注册 |

## 修改历史

### 2026-02-09: 修复文档检测功能

**问题**：以管理员身份运行时无法检测到 Word/WPS 打开的文档

**原因**：
1. COM 连接受 UIPI 权限隔离限制
2. Python Sidecar 的错误信息未正确传递到前端

**修复**：
1. 新增纯 Rust 实现的 `get_open_documents_native` Tauri 命令
   - 使用 `EnumWindows` Win32 API 枚举窗口
   - 使用 `CreateToolhelp32Snapshot` 检测进程
   - 不依赖 COM，不受权限限制
2. 前端优先使用 Rust 原生命令检测文档
3. 当文档没有完整路径时，弹出文件选择对话框让用户手动选择
4. 改善了错误提示信息
