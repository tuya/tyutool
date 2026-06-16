# 批量烧录工具设计文档

**日期**: 2026-06-01  
**分支**: `yj/toolbox`  
**状态**: 待实现

---

## 需求概述

在现有 tyutool 工具箱中新增批量烧录工具，支持同时向多个串口并行烧录同一份固件。

**核心约束**：
- 最多 32 个串口并行烧录
- 所有设备共用同一套配置（芯片型号、波特率、固件路径）
- 某个端口失败，其他继续运行，互不影响
- 仅 GUI，不需要 CLI 支持

---

## 路由与导航

侧边栏新增"工具箱"导航项，当前阶段直接重定向到批量烧录页面。待工具增多后再补充工具箱落地页。

```
/toolbox                    → ToolboxPage（工具卡片落地页）
/toolbox/batch-flash-auth    → 批量烧录授权页面
/toolbox/batch-flash        → redirect /toolbox/batch-flash-auth（兼容旧书签）
```

> **命名说明（2026-06）**：前端 feature 正式名为 `batch-flash-auth`（对齐产品名「批量烧录授权」）。Rust Tauri 子能力 IPC（`batch_flash_*`、`batch_auth_*`）保持不变。

---

## 前端文件结构

```
src/features/toolbox/
├── ToolboxPage.vue
├── tools.ts                    # 工具注册表（route / i18n / icon）
└── components/
    └── ToolboxBreadcrumb.vue

src/features/batch-flash-auth/
├── BatchFlashAuthPage.vue      # 页面入口
├── components/
│   ├── BatchFlashAuthConfig.vue
│   ├── BatchAuthConfig.vue     # 批量授权配置区（仅 ESP32 时显示）
│   ├── BatchFlashAuthDashboard.vue
│   ├── BatchFlashAuthToolbar.vue
│   ├── BatchFlashAuthSlotList.vue
│   ├── BatchFlashAuthSlotRow.vue
│   ├── BatchFlashAuthProgressBar.vue
│   └── BatchFlashAuthPortFilterModal.vue
├── types.ts
├── port-filter.ts
└── index.ts

src/stores/batch-flash-auth.ts          # Pinia store
src/stores/batch-flash-auth-workspace.ts
```

---

## UI 布局

**两个独立条件，不要混淆：**

| 条件 | 控制什么 |
|---|---|
| 所选芯片 ∈ {ESP32, GD32} | 是否显示 `BatchAuthConfig` 区域（这些芯片才支持批量授权协议） |
| Excel 文件是否填写 | `opMode`（决定执行什么操作） |

**完整逻辑链**（代码层面）：

```ts
// 1. 支持授权的芯片列表（单一修改点，扩展时只改这里）
const BATCH_AUTH_SUPPORTED_CHIPS = ['ESP32', 'GD32'] as const

// 2. 芯片支持 → BatchAuthConfig 区域可见（Excel 选择器才出现）
const authSupported = computed(() =>
  BATCH_AUTH_SUPPORTED_CHIPS.includes(selectedChipId.value as any)
)

// 3. 有 Excel（且芯片支持）→ opMode 包含授权步骤
const opMode = computed<BatchOpMode>(() => {
  const hasFirmware = !!firmwarePath.value
  const hasExcel = authSupported.value && !!authConfig.value.excelPath
  if (hasFirmware && hasExcel) return 'flash-then-auth'
  if (hasExcel) return 'auth-only'
  return 'flash-only'
})

// 4. 授权统计仪表板显示条件 = opMode 包含授权步骤
const showAuthStats = computed(() => opMode.value !== 'flash-only')
```

串联关系：芯片列表控制 **UI 入口**（Excel 选择器是否出现）→ 用户填 Excel → opMode 变化 → 授权统计面板出现。三者是顺序依赖，不是并列条件。

操作模式由填写内容决定，不需要 Tab 或模式切换按钮：

| 固件文件 | Excel 文件（且芯片支持授权） | 执行动作 |
|---|---|---|
| ✓ | — | 仅批量烧录 |
| ✓ | ✓ | 先烧录再授权 |
| — | ✓ | 仅批量授权 |

```
┌──────────────────────────────────────────────────────────┐
│  页面标题（复用 page-header 样式）                          │
├──────────────────────────────────────────────────────────┤
│  仪表板                                                    │
│  ┌─ 烧录累计 ──────────────────────────────────────────┐  │
│  │  [████████████████████░░░░░░]  82%                 │  │
│  │  绿=成功 / 红=失败   总47  ✓38  ✗9            [重置] │  │
│  └─────────────────────────────────────────────────────┘  │
│  ┌─ 授权累计（opMode 包含授权步骤时显示）────────────────────┐  │
│  │  [█████████████████░░░░░░░░░]  72%                 │  │
│  │  总25  ✓18  ✗7                               [重置] │  │
│  └─────────────────────────────────────────────────────┘  │
│  本次进度  进行中 3 · 成功 5 · 失败 1 · 已用时 00:01:23    │
├──────────────────────────────────────────────────────────┤
│  共享配置（任务进行中锁定只读）                              │
│  芯片选择  |  波特率  |  固件文件路径（可选）                │
├──────────────────────────────────────────────────────────┤
│  批量授权配置（仅 ESP32/GD32 时显示）                       │
│  授权表 .xlsx  [浏览]   总计 100 · 已用 38 · 剩余 62       │
│  ⚠ 遇到已授权设备：[跳过 ▾]（跳过 / 覆盖）                 │
├──────────────────────────────────────────────────────────┤
│  操作栏                                                    │
│  [自动分配 开关]  [⚙ 串口过滤 ②]    [开始] [取消] [重试失败]│
│  左组：功能性控件                      右组：动作按钮        │
├──────────────────────────────────────────────────────────┤
│  端口列表（可滚动，行高固定 h-10）                           │
│  port    状态              进度              操作           │
│  COM3    [烧录中]          ████░░ 68%        [取消]        │
│  COM5    [写入授权]        ████████ 85%      [取消]        │
│  COM7    [成功]            ██████ 100%                     │
│  COM9    [失败] ⓘ 错误    ░░░░░░              [重试] [删除] │
│  COM11   [空闲]                                    [删除]  │
└──────────────────────────────────────────────────────────┘
```

---

## Pinia Store（store.ts）

### 类型定义

```ts
// 镜像 Rust BatchFlashProgressEvent
interface BatchFlashProgressEvent {
  port: string
  event: FlashProgressPayload  // 复用现有单烧类型
}

// 授权进度事件
interface BatchAuthProgressEvent {
  port: string
  step: 'reading_mac' | 'reading_auth' | 'writing_auth' | 'verifying' | 'done' | 'failed' | 'skipped'
  mac?: string      // read_mac 成功后填入
  error?: string
}

type BatchOpMode = 'flash-only' | 'auth-only' | 'flash-then-auth'

interface BatchSlotState {
  port: string
  status: 'idle' | 'flashing' | 'reading_mac' | 'authorizing' | 'done' | 'failed' | 'skipped'
  progress: number      // 0–100
  currentPhase: string  // 当前阶段描述，显示在状态列
  mac?: string          // read_mac 成功后缓存，用于日志
  error?: string        // 完整错误信息，UI 通过 tooltip/展开显示
}

// 持久化到 plugin-store，key: 'batch-flash-auth-cumulative'（兼容读取旧 key 'batch-flash-cumulative'）
interface CumulativeStats {
  flash: { total: number; success: number; fail: number }
  auth:  { total: number; success: number; fail: number }
}

// 持久化到 plugin-store，key: 'batch-flash-auth-port-filter'（兼容读取旧 key 'batch-flash-port-filter'）
interface PortFilterConfig {
  blockedPorts: string[]  // Windows 存储时规范化为大写，Linux 区分大小写
}

// 批量授权专用配置
interface BatchAuthConfig {
  excelPath: string
  conflictPolicy: 'skip' | 'overwrite'  // 遇到已授权设备的处理策略，默认 skip
}

// 操作模式由填写内容推导，不需要显式字段
const opMode = computed<BatchOpMode>(() => {
  const hasFirmware = !!firmwarePath.value
  const hasExcel = !!authConfig.value.excelPath
  if (hasFirmware && hasExcel) return 'flash-then-auth'
  if (hasExcel) return 'auth-only'
  return 'flash-only'
})
```

### 关键 computed

```ts
// 本次统计，从 slots 派生，不持久化
const currentStats = computed(() => ({
  flashing: slots.value.filter(s => s.status === 'flashing').length,
  done:     slots.value.filter(s => s.status === 'done').length,
  failed:   slots.value.filter(s => s.status === 'failed').length,
  skipped:  slots.value.filter(s => s.status === 'skipped').length,
}))

// 操作栏按钮启用状态
const canStart   = computed(() => slots.value.some(s => s.status === 'idle') && inputsValid.value)
const canCancel  = computed(() => slots.value.some(s => ['flashing', 'reading_mac', 'authorizing'].includes(s.status)))
const canRetry   = computed(() => slots.value.some(s => s.status === 'failed'))
const filterActive = computed(() => filterConfig.value.blockedPorts.length > 0)

// 输入校验：flash-only / flash-then-auth 需固件；auth-only / flash-then-auth 需 Excel
const inputsValid = computed(() => {
  if (opMode.value !== 'auth-only' && !firmwarePath.value) return false
  if (opMode.value !== 'flash-only' && !authConfig.value.excelPath) return false
  return true
})
```

### Slot 状态机

**批量烧录（flash-only）**：
```
idle → flashing → done | failed
```

**仅批量授权（auth-only）**：
```
idle → reading_mac → authorizing → done | failed | skipped
```
`skipped`：设备已授权且与 Excel 记录一致，或 `conflictPolicy=skip` 时设备已授权但不一致。

**先烧录再授权（flash-then-auth）**：
```
idle → flashing → reading_mac → authorizing → done | failed | skipped
```
烧录失败时直接进入 `failed`，不进入授权阶段。

---

## Tauri 后端（src-tauri/src/lib.rs）

### BatchFlashState

```rust
struct BatchSlot {
    cancel: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

struct BatchFlashState {
    // key = 规范化后的 port name
    slots: StdMutex<HashMap<String, BatchSlot>>,
}
```

### 新增命令（烧录）

#### `batch_flash_start(config, ports)`

- 仅接受处于 idle 或 failed 状态的端口（由前端保证传入）
- 对每个端口：若 HashMap 中已存在旧 slot（failed 的残留 handle），等待旧线程退出（最多 3 秒，超时则返回错误，与单烧策略一致）
- 为每个端口创建独立的 `Arc<AtomicBool>` cancel flag，spawn 线程
- 线程内：先验证固件文件存在（防止校验与执行之间的时间窗口问题），再调用 `tyutool_core::run_job`
- 进度通过 `batch-flash-progress` 事件回传

#### `batch_flash_cancel_port(port)`

向对应 slot 的 cancel flag 写 `true`，不 join 线程（线程自行退出后进入 failed）。

#### `batch_flash_cancel_all()`

遍历所有 slot，逐一 cancel。

### 新增命令（授权）

#### `batch_auth_start(config, ports)`

- `config` 包含：芯片 ID、波特率、Excel 路径、可选固件路径、`conflict_policy`（skip/overwrite）
- 和 `batch_flash_start` 共用 `BatchFlashState`（同一 HashMap，无需单独状态结构）
- 每个线程的授权流程：
  1. 若有固件：先调用 `tyutool_core::run_job` 完成烧录 + 复位
  2. 打开串口（授权波特率）
  3. 发送 `read_mac` 命令，解析 MAC
  4. 发送 `auth-read` 命令，读取当前授权
  5. **行分配**：从共享的 `ExcelRowAllocator` 获取一条未使用记录（内部 `Mutex<Workbook>` 保证原子性，避免两线程分配同一行）
  6. 若设备已授权且与分配行一致 → 发 `skipped` 事件
  7. 若设备已授权且不一致：按 `conflict_policy` 决定覆盖或跳过
  8. 发送 `auth` 命令写入 UUID + AUTHKEY
  9. 发送 `auth-read` 验证回读
  10. 回写 Excel：标记 USED、写入 MAC 和时间戳
- 进度通过 `batch-auth-progress` 事件回传

#### `batch_auth_cancel_port(port)` / `batch_auth_cancel_all()`

与烧录取消共用同一机制（cancel flag 写 true）。

#### Excel 行分配器（Rust 内部结构）

```rust
struct ExcelRowAllocator {
    workbook: Mutex<Workbook>,   // 独占 .xlsx 文件访问
    next_row: Mutex<usize>,      // 当前分配游标
}
```

`allocate_row()` 在 `next_row` 上加锁，跳过 STATUS=USED 的行，返回该行的 UUID/AUTHKEY，并立即在内存中标记为"已分配（未确认）"。确认写入成功后再持久化 USED 标记。若所有行已用，返回 `Err`，slot 进入 `failed`（提示"授权码已耗尽"）。

### 进度事件

```
烧录事件: batch-flash-progress
Payload: { port: String, event: FlashProgressPayload }

授权事件: batch-auth-progress
Payload: { port: String, step: AuthStep, mac?: String, error?: String }

AuthStep: "reading_mac" | "reading_auth" | "writing_auth" | "verifying" | "done" | "failed" | "skipped"
```

与单烧的 `flash-progress` 事件名不同，两者完全独立。

### 应用关闭清理

在 `RunEvent::ExitRequested` 时调用 `batch_flash_cancel_all()`，随后 join 所有线程（最多等待 5 秒），确保串口锁和 Excel 文件锁释放后再退出。

---

## 自动分配串口

- 开关为**一次性触发**（非持续刷新）：打开开关 → 立即扫描一次 → 关闭开关
- 流程：调用 `list_serial_ports_cmd` → 过滤掉 `blockedPorts`（规范化后比较）→ 将新端口追加到 slot 列表（已有 slot 保留状态不变）
- 已拔出端口：不自动移除（保留 slot，status 维持原状），用户手动删除
- 开关关闭后：slot 列表不变

---

## 串口过滤

- 独立弹窗（`PortFilterModal.vue`）配置 `blockedPorts`
- 规则类型：精确匹配端口名
  - Windows：比较前统一转大写（`COM3`、`com3` 均可匹配）
  - Linux/macOS：区分大小写
- 过滤规则持久化（`plugin-store`），跨会话生效
- 规则生效时：操作栏过滤按钮显示数字徽标（`过滤 ②`）
- 配置变更后自动重新过滤当前 slot 列表中的 idle slot（flashing/done/failed 的不动）

---

## 错误处理

| 场景 | 处理 |
|---|---|
| 单个 slot 烧录失败 | slot 进入 failed，`error` 字段存储完整错误；UI 行内截断显示，ⓘ 图标 hover 展开完整信息 |
| 单个 slot 授权失败（read_mac / 写入 / 验证） | slot 进入 failed，错误信息含失败阶段；已分配但未确认的 Excel 行释放回"未使用"状态 |
| 授权码已耗尽（Excel 无剩余行） | slot 进入 failed，提示"授权码已耗尽，请补充 Excel" |
| 设备已授权 + 与表不一致 + policy=skip | slot 进入 skipped，不消耗授权码，提示"已跳过（设备已授权）" |
| 设备已授权 + 与表不一致 + policy=overwrite | 继续分配新行，覆盖写入并更新 Excel |
| 端口被单烧或串口调试占用 | 通过 `port-manager` 申请占用，申请失败时 slot 立即进入 failed，错误提示"端口已被占用" |
| 固件未选择或不存在 | 前端禁用"全部开始"按钮；Rust 侧每个线程启动前再次校验 |
| Excel 文件不存在或格式错误 | 前端校验，禁用"全部开始"并提示；Rust 侧加载时再次校验 |
| 自动分配结果为空（含过滤后） | 列表显示空状态提示 |
| 任务进行中修改共享配置 | 配置区字段禁用（只读） |
| 重试时旧线程未退出 | 等待最多 3 秒；超时则 slot 显示"端口未释放，请稍后重试" |
| 应用关闭时有任务在跑 | cancel all + join all（5 秒超时），串口和 Excel 锁释放后退出 |

---

## port-manager 集成

批量烧录的每个 slot 在启动时通过 `port-manager` 申请端口占用：
- 申请成功 → 正常烧录，slot done/failed 后释放
- 申请失败（已被占用）→ 立即 failed，错误提示"端口已被占用（被 [功能名] 占用）"

此机制防止与单烧页面或串口调试页面产生隐性冲突。

---

## UX 规则

| 场景 | 行为 |
|---|---|
| slot 数量 > 8 时点"全部开始" | 弹确认对话框，列出端口数、固件路径（如有）、Excel 剩余授权数（如有） |
| "重试失败"无 failed slot | 按钮禁用（不隐藏） |
| 删除 slot | idle 和 done 状态可删；flashing / reading_mac / authorizing / failed 禁止删除 |
| 累计统计重置 | 烧录和授权各有独立"重置"按钮，弹确认后清零并持久化 |
| 本次进度 | 批次开始时清零；显示已用时（精确到秒） |
| "全部开始" | 仅启动 idle slot；failed/skipped slot 不受影响（留给"重试失败"） |
| skipped slot | 不计入累计成功/失败；在本次进度中单独计数（可选展示） |
| 授权配置区 | 仅当所选芯片 ∈ {ESP32, GD32} 时显示 |
| 固件文件 | flash-only 或 flash-then-auth 时必填；auth-only 时隐藏或置灰 |

---

## UI 规范

- **颜色**：严格使用 `--ty-*` CSS 变量，禁止硬编码颜色值。状态色：成功 `var(--ty-success)`，失败 `var(--ty-danger)`，进行中 `var(--ty-primary)`，跳过 `var(--ty-text-muted)`
- **按钮 class**：使用项目现有 `.ty-btn-primary-solid` / `.ty-btn-secondary` 体系，**不使用** DaisyUI `btn btn-primary` / `btn btn-ghost` / `btn btn-warning`（DaisyUI oklch 颜色与 `--ty-*` 变量在 IDE 主题下会冲突）
- **状态标签**：基于 `--ty-*` 变量手写 `color` / `background-color`，**不使用** DaisyUI `badge-*` class
- **左边框粗细**：统一 `border-l-[3px]`，与现有 `.conn-bar` / `page-header` 的 `border-left: 3px` 规格一致，不用 `border-l-4`
- **进度条**：`BatchProgressBar.vue` 封装双色分段效果（绿=成功，红=失败），DaisyUI 原生 `progress` 不支持双色故需自定义
- **行高**：slot 列表每行固定 `h-10`，防止多 slot 并发更新时布局抖动
- **操作栏**：功能性控件（自动分配、过滤）左对齐；动作按钮（开始、取消、重试）右对齐，中间 `flex-1` 撑开
- **页面头部**：复用现有 `page-header` / `page-header-bg` / `page-header-icon` CSS 变量，与其他页面保持视觉一致

---

## UI 详细设计（视觉）

### 仪表板

累计统计使用 **SVG Donut Ring**（双段圆弧，零外部依赖），不使用图表库。本次进度使用 Stat Cards。

```
┌── 烧录统计 ─────────────────────────────┐  ┌── 本次批次 ─────────────┐
│  ╭────────╮                              │  │                          │
│  │  ●●●●  │  ✓ 成功  38   82%           │  │  ● 进行中   3            │
│  │ ● 82%● │  ✗ 失败   9   18%           │  │  ✓ 成功     5            │
│  │  ●●●●  │  总计    47        [重置]    │  │  ✗ 失败     1            │
│  ╰────────╯                              │  │  ⏱ 已用时   00:01:23     │
└─────────────────────────────────────────┘  └──────────────────────────┘

┌── 授权统计（opMode 包含授权步骤时显示）──────┐
│  （同上结构，独立 donut + 数字 + 重置）       │
└───────────────────────────────────────────────┘
```

**SVG Donut Ring 计算方式**（`BatchDonutChart.vue`）：

```
圆半径 r = 48，周长 C = 2π × 48 ≈ 301.59
成功弧长 = C × (success / total)，clamp 到 [0, C]
失败弧长 = C × (fail / total)，clamp 到 [0, C - successArc]
// 两段弧之和严格 ≤ C，防止浮点误差导致弧段重叠
背景圆用 var(--ty-border)；成功弧用 var(--ty-success)；失败弧用 var(--ty-danger)
SVG stroke-dasharray + stroke-dashoffset 实现双段，transform="rotate(-90 60 60)" 从顶部开始
```

总计为 0 时，显示完整灰色圆环 + 中间"-"占位。

### 端口列表行设计

每行固定 `h-10`，布局：`[左边框] [端口名] [状态标签] [迷你进度条 + 百分比] [操作按钮]`。

状态标签和左边框色均基于 `--ty-*` 变量手写，**不使用** DaisyUI badge/btn class（避免 oklch 颜色体系与项目 `--ty-*` 变量冲突）：

```
状态            左边框                       背景                       标签样式（inline style）
idle            无                           默认                       text-[var(--ty-text-muted)]
flashing        border-l-[3px] --ty-primary  无                         color: --ty-primary + 旋转点动画
reading_mac     border-l-[3px] --ty-primary  无                         color: --ty-primary  "读取MAC"
authorizing     border-l-[3px] --ty-primary  无                         color: --ty-primary  "写入授权"
done            border-l-[3px] --ty-success  无                         color: --ty-success
failed          border-l-[3px] --ty-danger   淡红 6% opacity            color: --ty-danger
skipped         border-l-[3px] --ty-text-muted 无                       color: --ty-text-muted
```

`reading_mac` 和 `authorizing` 单独显示（不合并为"auth阶段"），让用户可区分当前卡在哪一步。

failed 行淡红背景：`bg-[color-mix(in_srgb,var(--ty-danger)_6%,transparent)]`

**行内进度条**（`flashing` / auth 阶段时显示）：
- 高度 `h-1`，紧贴在端口名下方或占据进度列
- 颜色：`var(--ty-primary)`（烧录）/ `var(--ty-success)`（授权验证通过后）
- 百分比数字显示在进度条右侧，如 `68%`

错误信息：行内截断，末尾 ⓘ 图标，hover 展开 tooltip 显示完整内容（桌面专用交互）。

### 配置区域

```
┌── 共享配置 ──────────────────────────────────────────────────────┐
│  芯片 [▾]     波特率 [▾]     固件文件 [                  ] [浏览]│
└──────────────────────────────────────────────────────────────────┘

┌── 批量授权配置（左侧 accent 竖线，仅 ESP32/GD32）────────────────┐  ← border-l-[3px] border-[var(--ty-accent)]
│  授权表 [                                        ] [浏览]         │
│  ┌──────────────────────────────────────────────────────────────┐│
│  │  总计 100  ·  已使用 38  ·  剩余 62                          ││  ← 三个 stat pill
│  └──────────────────────────────────────────────────────────────┘│
│  遇到已授权设备  ● 跳过（推荐）  ○ 覆盖                           │
└──────────────────────────────────────────────────────────────────┘
```

### 操作栏

```
[⊕ 自动分配 ◯]   [⚙ 串口过滤 ②]          [▶ 全部开始]  [■ 取消]  [↺ 重试失败]
← 左组：功能性 →                  flex-1  ←────── 右组：动作 ──────────────────→
```

按钮语义（使用项目现有 `.ty-btn-*` class 体系，不使用 DaisyUI `btn-*`）：

| 按钮 | class | 禁用条件 | 禁用时 tooltip |
|---|---|---|---|
| 全部开始 | `.ty-btn-primary-solid` | 无 idle slot 或输入校验失败 | "请先选择固件" / "请先添加串口" 等具体原因 |
| 取消 | `.ty-btn-secondary` | 无运行中 slot | — |
| 重试失败 | `.ty-btn-secondary`（橙色文字） | 无 failed slot（禁用不隐藏） | — |
| 串口过滤 | `.ty-btn-secondary` + 条件 badge | 无 | — |
| 自动分配 | `toggle`（项目现有样式） | 任务进行中时禁用 | — |

"累计统计重置"按钮：使用 `text-[var(--ty-danger)] hover:underline` 文字链接样式，点击弹 `TyConfirmDialog` 确认后清零。

### 新增 Vue 组件

| 组件 | 说明 | 依赖 |
|---|---|---|
| `BatchDonutChart.vue` | SVG 双段圆弧，props: total/success/fail | 无外部库 |
| `BatchProgressBar.vue` | 双色横向分段条 | 无外部库 |
| `BatchAuthConfig.vue` | 授权配置区（含 Excel stat pills）| — |

不引入任何图表库，保持 zero-dependency 原则。

### 空态 UI（端口列表为空）

端口列表区域为空时，显示居中引导区（不是空白）：

```
        ╔═══════════════════════════════╗
        ║   暂无串口                     ║
        ║   点击「自动分配」扫描可用串口  ║
        ║   或手动从下拉框添加端口        ║
        ╚═══════════════════════════════╝
```

文字颜色 `var(--ty-text-muted)`，图标使用项目现有 FontAwesome 图标集。

### 批次完成通知

所有 slot 均达到终态（done / failed / skipped）时，仪表板区域顶部出现一条完成横幅：

- **全部成功**：绿色背景横幅，"本次批次完成：{N} 台全部成功"
- **部分失败**：黄色背景横幅，"本次批次完成：{done} 成功，{failed} 失败，可点击「重试失败」"
- **全部失败**：红色背景横幅，"本次批次全部失败，请检查连接后重试"

横幅可手动关闭（×），不自动消失。颜色使用 `var(--ty-success)` / `var(--ty-accent)` / `var(--ty-danger)` 对应背景（8% opacity）。

### Excel 文件校验反馈

选择 Excel 文件后立即校验，结果显示在文件路径输入框下方：

| 情况 | 提示 |
|---|---|
| 加载成功 | 显示"总计 N · 已用 M · 剩余 K"stat pills（绿色剩余数） |
| 文件不存在 | 红色提示"文件不存在" |
| 格式错误（非 xlsx） | 红色提示"请选择 .xlsx 格式文件" |
| 缺少必要列 | 红色提示"缺少必填列：UUID / AUTHKEY" |
| 剩余授权数为 0 | 橙色警告"授权码已全部使用，请补充 Excel" + 禁用"全部开始" |

---

## 测试策略

### 前端单元测试（vitest）

| 文件 | 覆盖 |
|---|---|
| `store.test.ts` | slot 状态机转换（含授权阶段）；stats computed；重试只传 failed 端口；total 仅在终态递增；opMode 推导逻辑 |
| `port-filter.test.ts` | 过滤规则应用；Windows 大小写规范化；空规则透传 |

Tauri IPC 调用 mock 掉，只测纯逻辑。

### Rust 单元测试

- `BatchFlashState` 的 cancel 信号传递、slot 替换、重试等待逻辑
- `ExcelRowAllocator` 的并发行分配（多线程同时调用 `allocate_row`，验证无重复分配）、授权失败后行释放

### 手动验收

| 场景 | 验收标准 |
|---|---|
| 3 个端口并行烧录 | 进度条各自独立更新，互不干扰 |
| 1 个端口失败，其余继续 | 失败 slot 显示错误，其他正常完成 |
| 重试失败 | 仅失败端口重新烧录，成功端口不变 |
| 过滤规则生效 | 自动分配时屏蔽端口不出现；操作栏显示徽标 |
| 配置锁定 | 任务进行中，共享配置不可修改 |
| 累计统计跨会话 | 关闭再打开 app，烧录和授权累计数据各自保留 |
| 取消不计入统计 | 取消的 slot 不影响 total / success / fail 计数 |
| 应用关闭清理 | 烧录/授权中关闭 app，串口和 Excel 正常释放 |
| 批量授权 - 并发行分配 | 多个端口同时授权，Excel 中每行 UUID 仅被分配给一台设备 |
| 批量授权 - 授权失败行释放 | 授权失败的端口所分配的行恢复为未使用状态 |
| 批量授权 - 授权码耗尽 | 剩余行不足时，超额的 slot 进入 failed 并提示耗尽 |
| 批量授权 - 已授权跳过 | conflictPolicy=skip 时，已授权设备进入 skipped，不消耗授权码 |
| 先烧录再授权 | 烧录完成后自动进入授权流程；烧录失败时直接 failed，不进入授权 |
| ESP32/GD32 芯片 | 授权配置区显示；其他芯片时隐藏 |

---

## 批量授权补充说明

### 支持芯片

当前阶段：**ESP32**、**GD32**。在前端以常量 `BATCH_AUTH_SUPPORTED_CHIPS = ['ESP32', 'GD32']` 维护，是唯一修改点：

- 控制 `BatchAuthConfig` 区域是否对当前所选芯片可见
- 控制 `opMode` 计算中 `hasExcel` 的有效性（非支持芯片时 hasExcel 强制为 false）
- 控制授权统计仪表板的显示

后续新增支持芯片只需在此常量追加，不需要改其他逻辑。

### Excel 授权表格式

- 第一行为表头，列名不区分大小写
- 必填列：`UUID`、`AUTHKEY`（或 `key`）
- 可选列：`STATUS`、`MAC`、`TIMESTAMP`（不存在时工具自动追加）
- 空行跳过；`STATUS=USED`（不区分大小写）视为已使用
- 行分配：从第 2 行起，取第一条 UUID 和 AUTHKEY 均非空且未标记 USED 的记录

### Excel 文件安全

- Rust 侧持有 `Mutex<Workbook>`，多线程行分配不会产生竞态
- 首次写入前生成 `.bak` 备份（仅辅助，不能替代用户主动备份）
- 不支持多个 tyutool 实例同时操作同一 Excel（前端校验；Rust 侧打开失败时 slot 立即 failed）

### 授权成功后 Excel 回写

成功验证后更新对应行：
- `STATUS` → `USED`
- `MAC` → 设备上报的 MAC
- `TIMESTAMP` → 当前 UTC 时间（ISO 8601）

### 日志

每次授权批次在 Excel 所在目录生成独立日志：`表名_auth_时间戳.log`，路径在页面上可复制。
