# 日志分层设计：用户可见 vs 开发者可见

**日期：** 2026-05-19  
**状态：** 待实现  
**v2：** 补充 Opus 审查发现的缺口（LogKey/LogLine 迁移表、serde 格式、JobSummary 模式建模、多段 timeline、Cancelled 变体、WebSocket special case）

---

## 背景与问题

tyutool 支持 CLI、GUI（Tauri）、Web/IDE（WebSocket）三种前端。当前所有前端共享
`tyutool-core`，但日志输出混乱：

- `tyutool-core` 的 `log::info!`（协议细节、内部状态）与用户进度输出混在同一 stderr
- `FlashProgress::LogLine` / `LogKey` 在 CLI 中被静默忽略
- `FlashPhase` 用裸字符串传递，CLI 靠手工 `map_phase()` 映射，脆弱
- GUI 设置中的"日志等级"语义不清晰
- 没有固件大小、里程碑等用户需要的结构化事件

---

## 核心原则：两条通道，互不干涉

```
tyutool-core
    │
    ├─► FlashEvent callback  →  用户可见（CLI终端 / GUI界面 / WebSocket）
    └─► log::* macros        →  开发者诊断（文件，可选终端 --verbose）
```

**判断标准：** 问自己「用户看到这条信息，能做出什么决策？」

| 能 → FlashEvent | 不能 → log::* |
|---|---|
| 操作进行到哪个阶段 | 协议帧内容、字节地址 |
| 关键里程碑（连接成功、擦除完成） | 重试次数、内部状态变更 |
| 固件大小、操作结果 | 中间计算结果、调试细节 |
| 用户需要采取行动的提示 | 任何开发者才关心的细节 |

---

## Section 1：`FlashEvent` 类型设计（替换 `FlashProgress`）

在 `crates/tyutool-core/src/flash_event.rs` 中定义。所有变体使用结构体形式以兼容
`#[serde(tag = "kind")]`：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlashEvent {
    JobSummary(JobSummary),
    Phase { phase: FlashPhase },
    Percent { value: u8 },
    Milestone { milestone: FlashMilestone },
    /// 用户需要看到并可能需要采取行动的警告（如 LN882H 要求用户按住 BOOT 引脚）
    Warning { message: String },
    Done { result: FlashResult },
}
```

### 1.1 `JobSummary` — 模式感知设计

不同 `FlashMode` 需要展示的信息差异很大，用 `JobDetails` 枚举区分：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSummary {
    pub port: String,
    pub baud: u32,
    pub device: Option<String>, // Authorize 模式为 None
    pub details: JobDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobDetails {
    Flash {
        firmware_path: String,
        firmware_size: Option<u64>,
        range_start: String,
        range_end: String,
    },
    Read {
        output_path: String,
        range_start: String,
        range_end: String,
    },
    Erase {
        range_start: String,
        range_end: String,
    },
    Authorize {
        /// true = 写入授权，false = 只读当前授权
        write: bool,
    },
}
```

### 1.2 `FlashPhase` — 强类型阶段

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlashPhase {
    Handshake,
    ReadFlashId,
    Unprotect,
    Erase,
    /// 多段 flash 的段级阶段；单段直接用 Write + Erase
    WriteSegment { current: u32, total: u32 },
    Write,
    Verify,
    Protect,
    Reboot,
    Read,
    Save,
    LoadRam,
    SwitchBaud,
    Connect,
    /// 兜底：新增阶段应先扩展枚举，Other 仅作临时占位
    Other(String),
}
```

### 1.3 `FlashMilestone` — 关键里程碑

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlashMilestone {
    HandshakeComplete,
    /// chip_info：芯片型号 + 版本（来自 ESP 设备信息；Beken 类芯片为 None）
    Connected { chip_info: Option<String> },
    FlashIdRead { mid: Option<u32> },
    EraseComplete,
    SegmentWritten { current: u32, total: u32 },
    WriteComplete,
    VerifyPassed,
    Rebooted,
    /// TuyaOpen 授权读取结果（含敏感信息，GUI 需用安全弹窗展示）
    AuthReadComplete { uuid: String, authkey: String },
}
```

### 1.4 `FlashResult`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlashResult {
    Ok { elapsed_secs: f64 },
    Err { message: String, elapsed_secs: f64 },
    /// 用户主动取消（Ctrl+C 或 GUI 取消按钮）
    Cancelled { elapsed_secs: f64 },
}
```

### 1.5 Serde 线格式示例

所有端（CLI serve、Tauri 事件、WebSocket）使用同一 JSON 格式：

```json
// JobSummary - Flash 模式
{"kind":"job_summary","port":"/dev/ttyUSB0","baud":921600,"device":"BK7231N",
 "details":{"type":"flash","firmware_path":"firmware.bin","firmware_size":1877952,
            "range_start":"0x00000000","range_end":"0x001CE400"}}

// JobSummary - Authorize 模式（写入）
{"kind":"job_summary","port":"/dev/ttyUSB0","baud":115200,"device":null,
 "details":{"type":"authorize","write":true}}

// Phase - 简单阶段
{"kind":"phase","phase":"handshake"}
{"kind":"phase","phase":"erase"}

// Phase - 多段 flash
{"kind":"phase","phase":{"write_segment":{"current":1,"total":3}}}

// Percent
{"kind":"percent","value":42}

// Milestone - 无参数
{"kind":"milestone","milestone":"erase_complete"}

// Milestone - 带参数
{"kind":"milestone","milestone":{"connected":{"chip_info":"ESP32-D0WDQ6 (revision v3.0)"}}}
{"kind":"milestone","milestone":{"segment_written":{"current":1,"total":3}}}
{"kind":"milestone","milestone":{"auth_read_complete":{"uuid":"xxx","authkey":"yyy"}}}

// Warning
{"kind":"warning","message":"Device not in ROM download mode — hold BOOT/A9 pin LOW, then power-cycle the device"}

// Done
{"kind":"done","result":{"ok":{"elapsed_secs":3.2}}}
{"kind":"done","result":{"err":{"message":"CRC mismatch at 0x001CE000","elapsed_secs":1.5}}}
{"kind":"done","result":{"cancelled":{"elapsed_secs":0.8}}}
```

---

## Section 2：`FlashProgress` → `FlashEvent` 迁移表

### 2.1 LogKey 迁移

| 当前 key | 插件 | params | 分类 | 迁移目标 |
|----------|------|--------|------|---------|
| `flash.log.segmentLog` | bk7231n, esp | `n` | 用户可见 | 由 `Phase(WriteSegment{current,total})` 取代，删除 LogKey 调用 |
| `flash.log.beken.readRange` | bk7231n | `start`,`end`,`kib` | 冗余 | 已在 `JobSummary::Read.range_*` 中体现，改为 `log::info!` |
| `flash.log.beken.savingBytes` | bk7231n | `size`,`path` | 冗余 | `Phase(Save)` 表达意图，路径在 `JobSummary` 中，改为 `log::info!` |
| `flash.log.esp.connected` | esp | `chip`,`revision` | 用户可见 | `Milestone(Connected { chip_info })` |
| `flash.log.esp.readDeviceInfoFailed` | esp | `error` | 错误路径 | 继续走异常，最终体现在 `Done(Err)` 中 |
| `flash.log.auth.readResult` | authorize | `uuid`,`authkey` | 用户可见（敏感） | `Milestone(AuthReadComplete { uuid, authkey })` |

### 2.2 LogLine 迁移

| 位置 | 当前消息 | 分类 | 迁移目标 |
|------|---------|------|---------|
| `ln882h:88` | "Device not in ROM download mode — hold BOOT/A9 pin LOW..." | 用户操作提示 | `Warning { message }` |
| `ln882h:216` | `"Reading 0x{start}..0x{end} ({n} bytes)"` | 冗余（JobSummary 已有范围） | `log::info!` |
| `ln882h:277` | `"Read complete: {} bytes saved to {path}."` | 由 Done(Ok) 覆盖 | `log::info!` |
| `ln882h:317` | `"Erasing 0x{start}..0x{end} ({n} bytes)"` | 冗余（JobSummary 已有范围） | `log::info!` |
| `ln882h:324` | `"Erase complete."` | 里程碑 | `Milestone(EraseComplete)` |
| `ln882h:371` | `"Segment {}/{}: erasing 0x{start}..0x{end}"` | 由 Phase 覆盖 | `log::info!` |
| `ln882h:390` | `"Writing {} bytes..."` | 冗余 | `log::info!` |
| `ln882h:408` | `"Segment {}/{} written ({total_bytes} bytes)."` | 里程碑 | `Milestone(SegmentWritten { current, total })` |
| `bk7231n:48` log 闭包 | 各种内部状态消息 | 开发者诊断 | `log::info!` |

---

## Section 3：多段 flash 事件时间线

以 BK7231N 双段固件为例，完整事件序列：

```
JobSummary(Flash, firmware.bin 1.8MiB, BK7231N, /dev/ttyUSB0, 0x0→0x1CE400)
Phase { phase: "handshake" }
Percent { value: 0..100 }
Milestone { milestone: "handshake_complete" }
Phase { phase: "read_flash_id" }
Milestone { milestone: { "flash_id_read": { "mid": null } } }
Phase { phase: "unprotect" }

-- 第 1 段 --
Phase { phase: { "write_segment": { "current": 1, "total": 2 } } }
Phase { phase: "erase" }
Percent { value: 0..100 }
Milestone { milestone: "erase_complete" }
Phase { phase: "write" }
Percent { value: 0..100 }
Milestone { milestone: { "segment_written": { "current": 1, "total": 2 } } }

-- 第 2 段 --
Phase { phase: { "write_segment": { "current": 2, "total": 2 } } }
Phase { phase: "erase" }
...
Milestone { milestone: { "segment_written": { "current": 2, "total": 2 } } }

Phase { phase: "verify" }
Percent { value: 0..100 }
Milestone { milestone: "verify_passed" }
Phase { phase: "reboot" }
Milestone { milestone: "rebooted" }
Done { result: { "ok": { "elapsed_secs": 5.4 } } }
```

**消费者状态追踪规则：** `Percent` 不携带当前阶段信息，消费者（CLI/GUI）追踪最近一次
`Phase` 事件作为 Percent 的归属阶段。这是有意识的设计决策（保持 Percent 轻量），
各端独立维护当前阶段状态。

---

## Section 4：CLI 设计

### 启动 banner

一行，打到 stderr，不使用 `log::info!`：

```
tyutool v3.0.7  linux/x86_64
```

### 开发者日志路由

```
默认：log::* → {data_dir}/tyutool/tyutool.log（自动轮转）
--verbose：同时也打到 stderr
```

日志文件路径使用 `dirs::data_dir()` 解析，跨平台：
- Linux：`~/.local/share/tyutool/tyutool.log`
- macOS：`~/Library/Application Support/tyutool/tyutool.log`
- Windows：`%APPDATA%\tyutool\tyutool.log`

`--verbose` 启动时打印：`[log] Writing to: ~/.local/share/tyutool/tyutool.log`

日志格式：`[2026-05-19 10:23:45 INFO tyutool_core::plugins::beken::ops] Starting handshake`

### `CliReporter` — rich 模式（TTY）

```
tyutool v3.0.7  linux/x86_64

write · BK7231N · /dev/ttyUSB0 @ 921600
  File   firmware.bin  1.8 MiB
  Range  0x00000000 → 0x001CE400

  ✓ Handshake complete
  ✓ Erase complete
  ⠸ Write [1/2]    ━━━━━━━━━░░░░░░░░░░░░░░  42%

⚠ Device not in ROM download mode — hold BOOT/A9 pin LOW, then power-cycle the device
```

- `FlashEvent::JobSummary` → 打印操作头（按 `JobDetails` 类型展示对应字段）
- `FlashEvent::Phase` → 进度条阶段切换，`FlashPhase` 枚举直接映射显示文本
- `FlashEvent::Percent` → 进度条更新
- `FlashEvent::Milestone` → `✓ <里程碑文字>`
- `FlashEvent::Warning` → `⚠ <message>` 打印在当前进度条下方
- `FlashEvent::Done(Ok)` → `✓ Flash complete  3.2s`
- `FlashEvent::Done(Err)` → `✗ Flash failed: <msg>  3.2s`
- `FlashEvent::Done(Cancelled)` → `✗ Cancelled  0.8s`

### `CliReporter` — plain 模式（非 TTY，CI/管道/重定向）

```
tyutool v3.0.7  linux/x86_64

write  BK7231N  /dev/ttyUSB0  921600
  File   firmware.bin  1.8 MiB
  Range  0x00000000 -> 0x001CE400

Handshake      OK
Erase          10%  20%  30%  40%  50%  60%  70%  80%  90%  OK
Write [1/2]    10%  20%  30%  40%  50%  60%  70%  80%  90%  OK
Write [2/2]    10%  20%  30%  40%  50%  60%  70%  80%  90%  OK
Verify         OK
Reboot         OK
Flash OK  3.2s
```

规则：
- 阶段名左对齐固定宽度（14字符），`OK` 结尾
- 长阶段（Erase / Write / Read）每 10% 行内 `eprint!`，完成后换行
- 短阶段（Handshake / Verify / Reboot 等）只打 `OK`，无百分比
- Warning 单独打一行：`[WARN] <message>`
- 失败时：`Flash FAILED: <msg>  3.2s`
- 取消时：`Flash CANCELLED  0.8s`
- 分隔符用 `->` 而非 `→`（ASCII only）

### Authorize 模式 CLI 输出

```
tyutool v3.0.7  linux/x86_64

authorize · /dev/ttyUSB0 @ 115200  [read-only]

  UUID:    abc123...
  AuthKey: def456...
```

---

## Section 5：GUI（Tauri）设计

### 开发者日志路由（不变，但语义明确）

`tauri-plugin-log` 配置保持现状：`log::*` → 文件（`LogDir`）+ Stdout。

设置页"日志等级"控件需加说明文字：`"开发者日志文件等级（不影响界面显示）"`。

### UI 只渲染 `FlashEvent`

Tauri 后端将 `FlashEvent` 序列化为 Tauri 事件推送前端，UI 不消费任何 `log::*` 内容：

```
FlashEvent::JobSummary   → 固件信息卡片（按 JobDetails 类型展示对应字段）
FlashEvent::Phase        → 步骤指示器高亮当前阶段
FlashEvent::Percent      → 进度条
FlashEvent::Milestone    → 时间线打勾
FlashEvent::Warning      → 黄色警告横幅（内嵌在操作面板中）
FlashEvent::Done(Ok)     → 成功状态 + 耗时
FlashEvent::Done(Err)    → 错误状态 + 错误信息
FlashEvent::Done(Cancelled) → 已取消状态
```

`AuthReadComplete` 里程碑：GUI 以安全弹窗展示 uuid/authkey，不得以普通日志行展示。

UI 布局示意：

```
┌─────────────────────────────────────────┐
│ BK7231N  /dev/ttyUSB0  921600 baud      │
│ firmware.bin  1.8 MiB  0x00000000→...   │
├─────────────────────────────────────────┤
│ ✓ Handshake complete                    │
│ ✓ Erase complete                        │
│ ⟳ Writing...          ████░░░░  42%     │
└─────────────────────────────────────────┘
```

### 串口变化提示（前端层，不经过 core）

串口刷新检测已在 `port-manager` store 处理，UI 层直接展示 toast / 状态栏提示：
`"串口列表已更新"`，不需要 `FlashEvent` 承载。

---

## Section 6：Web/IDE 模式（`tyutool serve`）

`serve` 模式通过 WebSocket 透传 `FlashEvent` JSON，Web 前端消费同一套事件，
渲染逻辑复用 GUI 组件。开发者日志不走 WebSocket，只写 CLI 端文件。

### `file_content` 特殊消息（保持现状）

`serve.rs` 在 read job 完成后会发送一条非 `FlashEvent` 的特殊消息，携带 base64
编码的 flash 读取数据：

```json
{"kind": "file_content", "name": "flash_read.bin", "content": "<base64>"}
```

这条消息不属于 `FlashEvent`，由 `serve.rs` 在 `Done(Ok)` 之后单独发送，前端单独处理。
实现时保持此行为，不纳入 `FlashEvent` 枚举（数据传输不是日志）。

---

## Section 7：CLAUDE.md 规范条目

以下内容新增到项目 `CLAUDE.md`：

### 日志分层契约（Logging Contract）

tyutool 有两条独立通道，必须严格分离：

**用户可见：FlashEvent 回调**

凡是用户需要感知的信息，必须通过 `FlashEvent` 回调发出：
- 操作元信息（固件大小、端口、设备、模式）→ `FlashEvent::JobSummary`
- 阶段切换 → `FlashEvent::Phase(FlashPhase::*)`（强类型，禁止裸字符串）
- 进度 → `FlashEvent::Percent`
- 关键里程碑 → `FlashEvent::Milestone(FlashMilestone::*)`
- 用户需要采取行动的提示 → `FlashEvent::Warning { message }`
- 最终结果 → `FlashEvent::Done`

**开发者可见：log::* 宏**

凡是用于诊断 bug 的信息，使用 `log::info!` / `log::debug!` / `log::warn!` / `log::error!`：
- 协议帧内容、字节地址、重试次数
- 内部状态变更、中间计算结果
- 任何用户不需要、开发者才需要的细节

**禁止事项：**
- 禁止用 `log::info!` 输出用户可见内容
- 禁止在 `FlashPhase` / `FlashMilestone` 中使用裸字符串（新增阶段先扩展枚举，`Other(String)` 仅作兜底，视为技术债）
- `AuthReadComplete` 里程碑数据禁止以普通文本形式显示（GUI 必须使用安全弹窗）

**各端路由：**

| 端 | FlashEvent | log::* |
|----|-----------|--------|
| CLI | CliReporter → stderr | 文件（`--verbose` 也打 stderr）|
| GUI | Tauri 事件 → UI | tauri-plugin-log → 文件（等级由开发者设置控制）|
| Web/IDE | WebSocket JSON → 浏览器 UI | CLI 端文件 |

### CLI 命令文档同步

`docs/cli.md` 是 CLI 的权威参考文档（本次新建并补充完整）。每当 CLI 命令发生以下变更，
必须同步更新 `docs/cli.md`：
- 新增子命令或参数
- 删除或重命名子命令或参数
- 修改参数默认值或行为

禁止只改代码不改文档。审查时，CLI 相关 PR 必须包含对 `docs/cli.md` 的变更。

---

## 实现顺序

1. `tyutool-core`：新建 `flash_event.rs`，定义 `FlashEvent` 及所有子类型（含 serde derives）
2. `tyutool-core`：按迁移表（Section 2）改造各 plugin 的 LogKey / LogLine 调用点
3. `tyutool-core`：`run_job` 签名切换到 `Fn(FlashEvent)`，`FlashError::Cancelled` 映射到 `Done(Cancelled)`
4. `tyutool-cli`：初始化文件日志（`dirs::data_dir()`），添加 `--verbose` flag，banner 简化
5. `tyutool-cli`：`CliReporter` 重写，处理所有 `FlashEvent` 变体（rich + plain，含 Warning、Cancelled）
6. `src-tauri`：Tauri 事件切换为 `FlashEvent` JSON，前端 TypeScript 类型同步更新
7. `src/`：Vue 组件按新事件变体渲染 UI，`AuthReadComplete` 使用安全弹窗，设置页日志等级添加说明文字
8. 新建 `docs/cli.md`，补充完整 CLI 参考（含所有子命令、参数、默认值、示例）
9. 更新 `CLAUDE.md`，添加日志分层契约和 CLI 文档同步规则
