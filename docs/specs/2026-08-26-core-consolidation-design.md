# 核心下沉设计：让上层回归瘦客户端

**日期：** 2026-08-26
**状态：** 待实现
**范围：** `tyutool-core` / `tyutool-cli` / `src-tauri` / `tyutool-bridge` 的职责边界与契约收敛
**实测基准：** 本文所有行数、行号、计数均实测于 `123a343`。初稿曾误量于一个未同步的
旧工作树（`e01d7f4` 之前），导致整组数字系统性偏移。**修改本文时请重测并更新此行的 commit。**

---

## 背景与问题

AGENTS.md 已经写明「`tyutool-core` is the single source of truth for flash logic」。
这条约定在**烧录逻辑**上守住了——芯片插件、`run_job`、串口读写都在 core。

但在烧录逻辑之外，它没有守住。实测各 crate 代码量：

| crate | 行数 | 定位 | 实际 |
|---|---:|---|---|
| `tyutool-core` | 17284 | 唯一真相源 | plugins 7891、`authorize.rs` 3335、`serial_debug.rs` 3288 |
| `tyutool-bridge` | 8698 | WS 助手 | 协议与安全为主，基本合理 |
| **`src-tauri`** | **5953** | **薄壳** | **不薄** |
| `tyutool-cli` | 2294 | 薄壳 | `reporter.rs` 562 是终端渲染，合理 |
| `tyutool-serve` | 1691 | 开发期 WS | — |

**上层四家合计 18636 行，超过 core 本身（17284）。** 一个真正的瘦客户端不该有 5953 行。

### 症状一：同一份逻辑写了三遍

```
crates/tyutool-bridge/src/main.rs:336   fn prune_log_files(dir: &Path)
crates/tyutool-cli/src/main.rs:361      fn prune_log_files(log_dir: &Path)
src-tauri/src/logs.rs:195               pub(crate) fn prune_log_files(log_dir: &Path)
```

`crates/tyutool-bridge/src/main.rs:66` 的注释自己写着
「Session log retention for this binary, **mirroring** `prune_log_files` in ...」——
这是被明确承认的复制。

三处实现的是**同一套规则**（删最旧直到降到上限内），但**常量各不相同**，而且是刻意的：

| 实现 | 文件数上限 | 总字节上限 | 文件名前缀 |
|---|---:|---:|---|
| `tyutool-cli/src/main.rs:285` | 100 | 100 MB | `tyutool-` |
| `tyutool-bridge/src/main.rs:69` | **20** | **50 MB** | **`tyutool-bridge-`** |
| `src-tauri/src/logs.rs` | 100 | 100 MB | `tyutool-` |

bridge 的注释说明了理由：它是常驻进程，不是交互工具，所以预算更小。

**所以 P0 不是直接合并，而是把上限与前缀参数化后再合并。** 否则会把 bridge 的
刻意选择抹掉。真正的问题是“一套规则写了三遍”，不是“三组常量不一致”。

### 症状二：能力鸿沟

| 能力 | GUI | CLI | bridge |
|---|:-:|:-:|:-:|
| 单设备烧录 / 读取 / 擦除 / 授权 | ✅ | ✅ | ✅ |
| 批量烧录 + 批量授权 | ✅ | ❌ | ❌ |
| Excel 授权表导入 / 导出 | ✅ | ❌ | ❌ |
| 日志列举 / tail / 导出 zip | ✅ | ❌ | ❌ |
| 导出时脱敏（`mask = true`） | ✅ | ❌ | ❌ |
| `BatchAuthTraceWriter` 凭据隔离记录 | ✅ | ❌ | ❌ |

后四项都是 AGENTS.md 当作**安全与可运维契约**写下的机制，但它们全部只有 GUI 一家实现。
CLI 做授权时不产生 `.trace` 记录；CLI 无法导出脱敏日志包。

### 症状三：契约声明了四遍

`FlashJob` 的字段清单，目前在四个地方各存在一份：

| 位置 | 形态 |
|---|---|
| `crates/tyutool-core/src/job.rs:27` | `FlashJob`，17 个字段（唯一真相源） |
| `crates/tyutool-cli/src/main.rs:37` | clap `Commands` enum，Write/Read/Erase 各重复一遍 device/port/baud/start |
| `crates/tyutool-cli/src/main.rs:585 / 644 / 696 / 746` | **四处**手写 `FlashJob { .. }` 字面量，每处把用不上的字段显式写成 `None` |
| `src/features/firmware-flash/flash-ipc-types.ts`（102 行） | 手工镜像 `FlashJob` + `FlashEvent` |

`main.rs:585` 为了发一个 authorize job 写了 11 行 `None`。
加字段时前三处漏改会编译报错（尚可），**第四处漏改不会报错**——这才是真风险。

### 正面样板

`crates/tyutool-core/src/diagnostics.rs` 只有 83 行，`log_session_banner` 被
cli / bridge / src-tauri 三家共用，AGENTS.md 明写「Never re-inline a per-platform banner」。
`prune_serial_debug_archives` 同理，正确地留在 `core/serial_debug.rs`。

**我们已经知道该怎么做，只是只做了这两处。** 本设计要把同一个做法推广到其余部分。

---

## 核心原则：一条可机械执行的判据

> **依赖 `AppHandle` / `Window` / `emit` → 留 `src-tauri`
> 依赖 clap / 终端渲染 → 留 `tyutool-cli`
> 依赖 WS 连接与会话状态 → 留 `tyutool-serve` / `tyutool-bridge`
> 其余全部（纯 std / 纯 serde / 纯 fs / 纯串口）→ 沉入 `tyutool-core`**

这条判据不需要讨论「这算不算业务逻辑」，只需要看类型签名里有没有平台类型。

**一条例外**：纯 std 但只服务单一前端的功能（如「在编辑器里打开日志文件」），
沉下去只是给 core 塞死代码。判据是「是否**可能**被第二个前端使用」，
不是「是否**技术上**可以下沉」。

---

## Section 1：现状审计

### 1.1 该沉但没沉

| 内容 | 现在的位置 | 行数 | 判据结论 |
|---|---|---:|---|
| `prune_log_files` | 三处各一份 | ~60×3 | 纯 fs，零平台依赖 |
| `pick_active_log` / `collect_log_files` / `list_log_files_impl` / `validate_log_filename` / `tail_bytes` / `read_log_tail_impl` / `prune_trace_files` | `src-tauri/src/logs.rs` | ~250 | 纯 fs |
| `build_report_info` / `mask_value_range` / `redact_log_content` / `write_logs_zip` / `gather_and_write_logs_zip` | `src-tauri/src/logs.rs:865-978` | ~120 | 纯字符串 + zip；承载 `mask` 安全契约 |
| `is_newer` / `platform_key` / `verify_sha256` / `extract_binary_from_tar_gz` / `extract_binary_from_zip` | `crates/tyutool-cli/src/update.rs:36/57/105/115/135` | ~120 | 纯逻辑。⚙ **注意：`src-tauri/src/updater.rs` 并无同类实现**（详见 P1） |
| 批量编排：多口并发调度、冲突策略、Excel 读写 | `src-tauri/src/batch.rs`(943) + `batch_auth.rs`(1151) | **2094** | 编排本身与平台无关 |
| `BatchAuthTraceWriter` | `src-tauri/src/batch.rs` + `logs.rs` | — | 纯 fs，且是 AGENTS.md 的凭据隔离机制 |
| `calamine` / `rust_xlsxwriter` 依赖 | 仅 `src-tauri/Cargo.toml` | — | Excel 解析是纯数据处理 |

### 1.2 不该沉，留在原地

| 内容 | 位置 | 理由 |
|---|---|---|
| 8 个 `#[tauri::command]` 包装 | `src-tauri/src/logs.rs` | 真 glue，本就该薄 |
| `detect_vscode` / `detect_sublime_text` / `detect_notepad_plus_plus` 等编辑器探测 | `src-tauri/src/logs.rs:471-739` (~250) | 纯 std，但只有 GUI 需要「在编辑器里打开日志」 |
| `reporter.rs` | `tyutool-cli` (562) | indicatif / console 终端渲染 |
| `window.rs` / tray / autostart | `src-tauri` / `tyutool-bridge` | 平台 UI |
| Origin allowlist / token grants / audit log | `tyutool-bridge` | bridge 独有的安全模型，AGENTS.md 明确其与 serve 不同 |
| 芯片插件 | `tyutool-core/src/plugins/` | 已在正确位置 |

### 1.3 待评估：一次下沉已经先发生过

`src-tauri/src/serial_debug.rs` 现为 **661 行**，`core/serial_debug.rs` 3288 行，
`tyutool-serve/src/lib.rs` 1691 行。

**关键背景：`e01d7f4` 已经把 serve 与 src-tauri 两份重复的 chunk bridge 抽成了
`core/serial_debug_bridge.rs`（699 行）**——从 serve 搬走 417 行、从 src-tauri 搬走 429 行。
那正是本设计倡导的下沉动作，**已经成功做过一次**，应当作为正面样板引用。

所以 P5 的前提不是“三者边界不清”，而是“共享部分已抽走，剩下的 661 行是否还有
可抽的”——量级比初稿估计的小得多，优先级相应下调。

---

## Section 2：目标结构

### 2.1 core 分域

```
tyutool-core/
  ├─ 设备域（现有，不动）
  │    plugin.rs / registry.rs / job.rs / serial.rs
  │    plugins/ / authorize.rs / serial_debug.rs
  │
  ├─ diagnostics.rs          ← 扩成「日志治理域」
  │    log_session_banner                      （已有，保持）
  │    prune_log_files / prune_trace_files     （三合一）
  │    pick_active_log / collect_log_files / list_log_files
  │    tail_bytes / read_log_tail / validate_log_filename
  │    redact_log_content / write_logs_zip / build_report_info
  │
  ├─ updater.rs              ← 新增：版本比较 / 平台键 / 解包（均来自 CLI）+ sha256_hex（唯一真重复项）
  │
  └─ batch/                  ← 新增：批量编排状态机
       mod.rs
       excel.rs              #[cfg(feature = "excel")]
```

### 2.2 Excel 必须 feature gate

`calamine` + `rust_xlsxwriter` 若无条件进入 core，会灌进 CLI、serve、bridge 三个
二进制。core 已有现成先例——`src-tauri/Cargo.toml` 里写着
`tyutool-core = { path = "...", features = ["libudev"] }`。照抄这个模式：

```toml
# crates/tyutool-core/Cargo.toml
[features]
# 不声明 default —— AGENTS.md 明写「tyutool-core declares no `default` feature」
excel = ["dep:calamine", "dep:rust_xlsxwriter"]

[dependencies]
calamine = { version = "0.26", features = ["dates"], optional = true }
rust_xlsxwriter = { version = "0.79", optional = true }
```

两个依赖必须标 `optional = true`，`dep:` 语法才成立。

CLI 若要支持批量授权表，显式开启 `features = ["excel"]`；bridge 不开。

### 2.3 目标行数

按本文自己给出的搬迁量逐项相减，不拍脑袋：

| crate | 现在 | P0–P2 后 | 再加 P4（batch） | 构成 |
|---|---:|---:|---:|---|
| `src-tauri` | 5953 | ~5520 | ~3430 | −P0 60 −P1 370；P4 再搬 2094 |
| `tyutool-cli` | 2294 | ~2294 | ~2294 | 只减重复的 `prune_log_files`，同时白拿新能力 |
| `tyutool-bridge` | 8698 | ~8640 | ~8640 | 只减 `prune_log_files` |
| `tyutool-core` | 17284 | ~17750 | ~19850 | 受下沉量累加 |

初稿写的「src-tauri → ~2500」算不出来：即使 P4 把 2094 行全搬走，也只到 ~3430。
剩下的主体是 46 个 Tauri 命令包装、事件桥接、~250 行编辑器探测与 `window.rs`——
那些按 §1.2 就该留在原地。

---

## Section 3：契约收敛

下沉解决「功能在哪」，契约收敛解决「参数声明几遍」。两者互补：
**只有功能都在 core，CLI 才可能表达 GUI 的每一个操作。**

### 3.1 `FlashJob` 成为强制契约

给 `FlashJob` 补 `Default` / `new()`，让上层用 struct update 语法构造：

```rust
// crates/tyutool-cli/src/main.rs
#[derive(clap::Args)]
struct WriteArgs { device: String, port: Option<String>, baud: Option<u32>, /* ... */ }

impl From<WriteArgs> for FlashJob {          // ← 唯一的搬运处
    fn from(a: WriteArgs) -> Self {
        FlashJob {
            mode: FlashMode::Flash,
            flash_start_hex: Some(a.start),
            firmware_path: Some(a.file),
            ..FlashJob::new(a.device, port, baud)
        }
    }
}
```

`main.rs:585 / 644 / 696` 三处字面量随之消失，那 12 行 `None` 变成 `..Default::default()`。

`clap-serde-derive` 可让同一 struct 同时 derive clap 与 serde，是**可选加分项**，
不是本设计的前提——手写 `From` 已经解决主要问题。

### 3.2 TS 类型从 Rust 生成

引入 `ts-rs`（MSRV 1.88），`cargo test` 时导出：

```rust
// crates/tyutool-core/src/job.rs
#[derive(Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct FlashJob {
    // ...
    #[serde(skip)]
    #[ts(skip)]                                  // ← confirm_overwrite 是闭包
    pub confirm_overwrite: Option<Box<dyn Fn(String, String) -> bool + Send>>,
}
```

`src/features/firmware-flash/flash-ipc-types.ts` 从手写文件退化为生成产物的再导出。

**AGENTS.md 中「Frontend types manually mirror the corresponding Rust types」这条约定应随之删除**——
不再需要人来遵守它。这条约定是否还在，是本项完成与否的可验证信号。

> **风险在哪里（实测 `flash_event.rs` 的 serde 属性）：**
>
> | 行 | 属性 | 类型 | tag 模式 | ts-rs 风险 |
> |---|---|---|---|---|
> | 8 | `tag = "kind"` | `FlashEvent` | **internally tagged** | 高，需实测 |
> | 39 | `tag = "type"` | `JobDetails` | **internally tagged** | 高，需实测 |
> | 63 | 仅 `rename_all` | `FlashPhase` | externally tagged | 低 |
> | 88 | 仅 `rename_all` | `FlashMilestone` | externally tagged | 低 |
> | 130 | 仅 `rename_all` | `FlashResult` | externally tagged | 低 |
>
> `{ write_segment: { current, total } }` 这类形状正是 **externally tagged** 的产物，
> 也是 ts-rs 支持最好的一档。初稿把它们当成高风险项，**把风险指反了**。
>
> 落地顺序：先纯 struct `FlashJob` → 再 3 个 externally tagged 枚举 →
> 最后才是 `FlashEvent` 与 `JobDetails` 这两个 internally tagged 的。

### 3.3 命令回显：`to_cli_command()`

```rust
impl FlashJob {
    /// 该 job 的等价 CLI 命令，用于日志与 issue 上报。
    /// 无法完整表示时返回 None。
    pub fn to_cli_command(&self) -> Option<String>;
}
```

出现在三处：

1. `run_job` 开头，随已有的 `JobSummary` 一起进 `log::info!`
   → 每份 `tyutool-<ts>.log` 都含一行可重放命令
2. GUI 报错弹窗增加「复制等价命令」按钮
3. `.github/ISSUE_TEMPLATE/bug_report.yml` 引导用户贴这一行

返回 `Option` 是刻意的：QGIS 的 *Copy as qgis_process Command* 明确标注
「某些参数组合无法表示成命令字符串」。诚实标注优于生成一条跑不通的命令。

**注意**：P4（批量下沉）完成前，批量操作没有等价 CLI 命令可回显，因为 CLI 尚无该能力。
命令回显在那之前只覆盖单设备场景。

### 3.4 类型命名：`Flash*` 前缀名不副实

`FlashJob` 不是只针对 flash 的。`FlashMode` 有四个变体，`Flash` 只是其中一个：

```rust
pub enum FlashMode { Flash, Erase, Read, Authorize }
```

于是 `Flash` 在同一个类型里承担两种粒度：作为**类型前缀**它泛指整个设备操作域，
作为**枚举变体**它指“写入固件”这一个动作。结果是 `FlashJob { mode: FlashMode::Authorize }`
这种写法——一个“烧录任务”的模式是“授权”。`FlashJob` 的 17 个字段里有
`authorize_uuid` / `authorize_key` / `authorize_storage` / `confirm_overwrite`，与 flash 无关。

#### 目标命名

命名规则：**任务本体是 `Device*`（对设备做什么），执行过程上报的一切是 `Job*`（这次运行发生了什么）。**

| 现在 | 改为 | 理由 |
|---|---|---|
| `FlashJob` | `DeviceJob` | 一次针对串口设备的一次性、可取消操作 |
| `FlashMode` | `DeviceOp` | “mode” 弱化了它的作用；它选定的是要执行的动作 |
| `FlashEvent` | `JobEvent` | 四种操作共用 |
| `FlashPhase` | `JobPhase` | 同上 |
| `FlashMilestone` | `JobMilestone` | 已含 `AuthReadComplete` / `AuthWriteSent` / `AuthConflict` 等纯授权里程碑 |
| `FlashResult` | `JobResult` | 同上 |
| `FlashError` | **`DeviceError`**（不是 `JobError`） | ⚙ `tyutool_bridge::JobError`（`lib.rs:237`）**已占用该名**且被 6 个测试文件 import。bridge 同时 use core 与自身类型，重名为 `JobError` 会造成同文件两个 `JobError` |
| `FlashPlugin` | `ChipPlugin` | **最准确的一项**：`Authorize` 在 `run_job` 里被特判，根本不进插件，所以它确实只服务芯片 |
| `FlashPluginRegistry` | `ChipPluginRegistry` | 同上 |

已核实：`DeviceJob` / `DeviceOp` / `DeviceError` / `ChipPlugin` / `ChipPluginRegistry`
全仓库**无占用**。而 `JobSummary`（`flash_event.rs:30`）与 `JobDetails`（:40）已在 core 内
使用 `Job*` 命名空间，所以 `JobEvent` / `JobPhase` / `JobMilestone` / `JobResult`
与它们并存反而比现状更一致。

**不改的两个**（它们名副其实）：

- `FlashSegment` —— 只用于 flash 写入分段（`segments` 字段、`ln882h::resolve_segments`）
- `FlashParams`（`plugins/beken/flash_table.rs`）—— SPI-NOR 芯片参数表（MID、sector_size、wp）

#### 线格式安全性（执行前必读）

实测 `job.rs` / `flash_event.rs` 的 serde 属性，均为 `rename_all` 作用于变体与字段，
**类型名从不出现在序列化结果里**：

| 改什么 | 是否影响线格式 |
|---|---|
| 类型名（`FlashJob` → `DeviceJob`） | 否。纯 Rust 内部重命名 |
| 枚举变体名（`FlashMode::Flash`） | **是**。出现为 `"flash"` |
| 字段名（`mode`、`chip_id`） | **是**。camelCase 后进 JSON |
| Tauri 事件名（`flash-progress`） | 独立字符串，与类型名无关 |

所以本节列出的重命名**全部是线格式安全的**；`mode` 字段名若一并改成 `op`，则会同时打破
`tyutool-bridge/PROTOCOL.md`、WS 消息与前端镜像类型，必须单独决策。默认不改字段名。

#### 执行时机：随 P2（契约收敛）之后，不单独做

引用规模（口径：`grep -rn --include=*.rs --include=*.ts <名> crates src-tauri src` 的匹配行数）：
`FlashEvent` **183**、`FlashJob` **113**、`FlashMode` **61**。
（初稿写的 `FlashJob` 84 是另一种口径——仅 `.rs`、仅词边界，实测为 83。比较时请统一口径。）
在 ts-rs 落地前，前端那 100+ 行手工镜像类型要人改；落地后它们是生成产物。
所以重命名排在 **P2 之后**作为一个独立提交，不提前、也不与逻辑改动混在一起。

#### 更深的问题：重命名解决不了

真正的模型缺陷是“授权靠假芯片 + 旁路挤进 flash 结构”：

- `run_job` 对 `FlashMode::Authorize` 特判，绕过整个芯片注册表
- 前端造了一个 `AUTH_ONLY_CHIP_ID = "other"` 的**假芯片**，`rustPluginId` 为 `"OTHER"`

改名只是让名字诚实，旁路还在。“授权是否应该有自己的 job 类型”是**开放问题**，
不在本设计范围内；若将来要动模型，应连同重命名一起做，而不是先改名再改模型。

#### 短期（可立即做，半小时）

在 `job.rs` 给 `FlashJob` 与 `FlashMode` 各补一句 doc comment，点破前缀歧义：
`Flash` 前缀指设备操作域，不是 `FlashMode::Flash` 那个动作。
现有注释“One flash/erase/read/authorize job”信息在，但没点破歧义。

---

## Section 4：明确不做的事

记录否决理由，避免后续反复讨论。

| 方案 | 否决理由 |
|---|---|
| **核心跑成守护进程，GUI 改瘦客户端** | `src-tauri/src/lib.rs:683` 注册的 46 个命令中，`logs::register_dialog_path`、`reset_main_window_layout`、`tauri-plugin-store`、文件对话框**无法搬入守护进程**，结果是命令表分裂成两半。串口是独占资源，多客户端并发是伪需求。真正的成本在安装 / 自启 / 升级 / 版本漂移 / 端口冲突 / 三套平台服务，是永久运维负担。`tyutool-bridge` 已覆盖「远程 web 客户端」这个唯一真实需求。参照 rclone：`rcd` 是可选模式，本体仍是直接执行的 CLI。 |
| **把 46 个 Tauri 命令重组成统一命令表** | `serial_debug_*` / `logs::*` / `updater::*` 本就是不同动作，强行统一是过度抽象（违反 AGENTS.md §2 Simplicity First）。 |
| **引入 `tauri-specta`** | v2 长期停留在 `2.0.0-rc.24`。生产项目不为省类型手工活押注 RC。`ts-rs` 只导出类型、不碰 IPC 层，风险低得多。 |
| **引入 TauRPC / rspc** | 要求把现有命令重组成 trait / router，改动面远超收益。 |
| **改动 `tyutool-serve` / `tyutool-bridge` 的协议** | 两者的消息枚举各自保留，只让 payload 类型指向同一批 core 结构。`validate_ws_origin` 按 AGENTS.md 要求「ported verbatim，do not rewrite」。 |

---

## Section 5：验收标准

1. `grep -c "FlashJob {" crates/tyutool-cli/src/main.rs` 结果为 `0`（当前值：**4**）
2. `grep -rn "fn prune_log_files" crates src-tauri` 只剩一处，位于 `tyutool-core`（当前值：3）
3. `cargo test -p tyutool-core` 产出 `src/bindings/*.ts`，且 `pnpm run build` 类型检查通过
4. `src/features/firmware-flash/flash-ipc-types.ts` 不再包含手写的 `FlashJobPayload`
5. 给 `FlashJob` 加一个新字段，**只改 `job.rs` 一处**，`cargo build` + `pnpm run build` 全绿
6. 真机跑一次烧录，日志中能找到 `to_cli_command()` 输出，复制出来可直接重放
   （⚙ 需真机，**进不了 CI 门禁**；且按 §3.3，批量下沉之前只覆盖单设备场景）
7. `cargo tree -p tyutool-cli | grep -c calamine` 为 `0`（未开 `excel` feature 时）
8. AGENTS.md 中「Frontend types manually mirror」一条已删除

---

## 实现顺序

阶段划分与状态以本表为准。每完成一个阶段，在这里标记，并同步检查 AGENTS.md 的「已知违规」清单。

| 阶段 | 动作 | 量级 | 收益 | 状态 |
|:---:|---|---|---|---|
| **P0** | `prune_log_files` 三合一 → `core/diagnostics.rs`（上限与前缀参数化） | 半天 | 消掉已被承认的复制；验证下沉路径可行 | ✅ 已完成 |
| **P1a** | 日志保留与读取下沉（~250 行） | 3–4 天 | 代码层面对 CLI 可用（接入另议，见 P6） | ✅ 已完成 |
| **P1b** | 报告头 / 脱敏 / zip 导出下沉（~120 行）+ `zip` feature | 合并计入 P1a | `mask` 安全契约变成单点实现 | ✅ 已完成 |
| **P2-1** | `FlashJob::new` + 四处字面量收敛 + `to_cli_command()` + 往返测试 | 3–5 天 | 契约收敛；**命令回显上线**（唯一用户可见变化） | ✅ 已完成 |
| **P2-2** | `ts-rs` 生成 TS 类型（**仅 `FlashJob` 家族**） | 2–3 天 | 前端手工镜像部分退役；CI 校验无 drift | ✅ 已完成 |
| ~~**P3**~~ | ~~updater 纯逻辑下沉~~ | — | — | ❌ **已取消**，见下 |
| **P4** | 批量编排下沉 + `excel` feature | **2–3 周，有风险** | 代码层面对 CLI 可用 | 待评估 |
| **P5** | 弹性归档创建两份合一 → `core/serial_debug.rs` | 低 | 消除一份曾造成手工移植负担的重复 | ✅ 已完成 |
| — | 修复 P5 暴露的 backfill 目录 bug（GUI 命中 fallback 时索引写错位置） | 半天 | 行为修复，含回归测试 | ✅ 已完成 |
| **P6** | 把已下沉的能力接成 CLI 子命令 + 同步 `docs/cli.md` | 半天 | **真正兑现「CLI 能用」** | ✅ 已完成 |

### 一个必须说清的区分：下沉 ≠ CLI 能用

初稿把 P1 的收益写成「CLI 白捡日志列举 / tail / 导出 zip / 脱敏」。
**这个表述会让人误以为能力已交付。**

P1a + P1b 完成后的实际状态是：那些函数**住在 `tyutool-core` 里、CLI 可以链接到**，
但 **CLI 没有任何子命令调用它们**。用户依然不能用 CLI 列举日志、导出脱敏包。

接入不是机械动作，它需要：

1. 设计子命令形态（`logs list` / `logs tail` / `logs export`？还是归到一个 `logs` 下？）
2. 按 AGENTS.md 的硬规则，**同一个 commit / PR 必须同步 `docs/cli.md`**
3. 决定 `export` 子命令要不要开 `zip` feature（开了 CLI 就会链接 zip）

所以它是一个**独立阶段 P6**，而不是 P1 的附带结果。
AGENTS.md 的「已知违规」里那条「the CLI cannot do them at all」，
**只有 P6 落地才能划掉**。

**P6 落地结果（已完成）：** 三个问题的实际答案是——

1. 形态取 `tyutool logs list / tail / export`，`--dir` 作为组内 global 参数，
   默认 CLI 自己的日志目录，指向别处即可读 GUI 的日志。
2. `docs/cli.md` 同 commit 更新（新增 `logs` 一节、目录、命令总表、Log files 一节的交叉引用）。
3. `zip` feature 开了。代价实测为零：`tyutool-cli` 的 updater 早已直接依赖 `zip = "2"`，
   开启后依赖图不新增 crate。

另外两条实现上的决定值得记一笔：

- **`logs` 加入 `quiet` 集合**（与 `usb-port-survey` / `completions` 并列）。理由不止是
  stdout 干净：若 `logs` 也开自己的会话日志，`logs list` 每次都会列出它自己刚刚创建的那个文件。
- **没有在 CLI 侧重写任何日志逻辑**。文件筛选、`.trace` 拒读、路径分隔符拒绝、脱敏、zip 打包
  全部落在 `tyutool_core::diagnostics` 的既有函数上，CLI 侧只有渲染与参数解析。
  回归测试锁住了其中两条安全契约（不列举/不读取 `.trace`；`--file` 拒绝路径分隔符）。

### updater 阶段被取消的完整经过

这一项被降级了两次，最后取消。记录全程，因为它是本设计里**唯一一个因为没查代码就写进计划的阶段**。

**第一次降级（P1 → P3）** —— 初稿理由写的是「两套版本比较规则合一」，实测后不成立：

```
$ grep -n "is_newer\|platform_key\|extract_binary" src-tauri/src/updater.rs
（无输出）
$ grep -rn "fn sha256_hex" src-tauri crates
src-tauri/src/lib.rs:327:pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
```

`src-tauri/src/updater.rs` 只做 `update_check` / `update_download` / `update_install`，
**不含版本比较、不含平台键、不含归档解包**。初稿是看到 `updater.rs` 有 524 行
就推断的，没有查内容。

**第二次，也就是取消** —— 派活前逐函数核实 `crates/tyutool-cli/src/update.rs`：

```
$ grep -rn "is_newer\|platform_key\|GzDecoder\|tar::Archive" --include=*.rs src-tauri/src
（一处都没有）
$ grep -c sha2 crates/tyutool-core/Cargo.toml
0
```

| 函数 | 归属 |
|---|---|
| `platform_key` / `is_newer` / `is_windows` | **CLI 独有** —— GUI 走 `tauri-plugin-updater`，不自己比较版本 |
| `extract_binary_from_tar_gz` / `_zip` | **CLI 独有**，且需 `flate2` + `tar` |
| `replace_self` | **CLI 独有**（`self-replace`） |
| `fetch_latest_json` / `download_bytes` | CLI 独有 |
| `verify_sha256` | 与 `lib.rs:327` 的 `sha256_hex` **部分**重叠 |

而那两个甚至不是同一个函数：

```rust
pub(crate) fn sha256_hex(bytes: &[u8]) -> String            // 算摘要
fn verify_sha256(data: &[u8], expected_hex: &str) -> bool   // 算 + 比
```

**唯一真正共享的是「算 SHA-256 并转小写十六进制」这 5 行。**

#### 取消理由

1. 按本设计自己的 crate 边界规则（及 AGENTS.md 的例外条款）——
   **只服务单一前端的纯 std 代码留在该前端**——`update.rs` 绝大部分本就该留在 CLI。
2. 剩下那 5 行下沉，代价是给 core 加 `sha2` 依赖、波及 5 个 consumer；
   或再造一个 feature，机械成本超过省下的 5 行。
3. SHA-256 是固定标准，两份实现**不可能产生行为漂移**，只有写法差异。

按 `docs/specs/2026-08-27-refactor-v3-normalization-design.md` 定的筛选判据
（有实测证据 + 不做有具体代价），**这一项两条都不满足**。

#### 重新打开的条件

若将来出现**第三个** SHA-256 调用点，或 GUI 改成自行处理归档与版本比较
（不再依赖 `tauri-plugin-updater`），则重新评估。

### P4（批量下沉）的风险须单列

2094 行**不是机械搬迁**。GUI 的 batch 深度依赖 Tauri 事件流做多口进度上报，
必须先把「不含 IO 的编排状态机」与「事件发射」拆开——这本质上是重写而非移动。
建议 P0–P2 完成、下沉路径经过验证后再评估，不要一开始就啃它。

P0–P2 合计约一周半，将 `src-tauri` 从 5953 降至约 **5520**（−P0 60 −P1 370）。
行数下降不是目的；目的是留下一条被验证过、可重复的下沉路径，并让 CLI 白拿
日志导出与脱敏能力。

---

## 附：外部方案参照

调研过的同类做法，供实现时对照。

| 项目 | 可借鉴点 |
|---|---|
| [probe-rs](https://github.com/probe-rs/probe-rs) | 同领域最佳参照：一个库撑起 CLI + cargo 子命令 + VS Code 扩展 + GDB server；VS Code 那条走标准 DAP 而非自造协议 |
| [espflash](https://github.com/esp-rs/espflash) | README 明确「用作库时 `default-features = false` 关掉 cli 模块，cli 模块不提供 SemVer 保证」——**库 API 与 CLI API 分开做版本承诺**，本仓库目前没有这条 |
| [rclone](https://rclone.org/rc/) | `rcd` 是可选模式，Web GUI 完全走 RC API，但本体仍是直接执行的 CLI |
| [QGIS Processing](https://docs.qgis.org/3.44/en/docs/user_manual/processing/toolbox.html) | *Copy as qgis_process Command*：GUI 对话框一键导出等价命令行，并诚实标注无法表示的情形 |
| Blender | *Copy Python Command*：每次点击都对应一条可复制的脚本命令 |
| [ts-rs](https://github.com/Aleph-Alpha/ts-rs) | `#[derive(TS)]` + `#[ts(export)]`，`cargo test` 时导出到 `TS_RS_EXPORT_DIR` |
| [clap-serde-derive](https://github.com/DPDmancul/clap_serde_derive) | 同一 struct 同时 derive clap 与 serde，字段自动包 `Option`，clap 覆盖 serde（可选加分项） |

---

## 相关文档

| 文档 | 关系 |
|---|---|
| `docs/specs/2026-08-27-refactor-v3-normalization-design.md` | **上层总纲**。本文是它七个维度中的一条主线；总纲回答「做不做、先做哪个」，本文回答「怎么做」 |
| `docs/plans/2026-08-26-core-consolidation.md`（待立） | 本文的实现计划，按 P0–P5 拆成 checkbox 任务，遵循仓库既有 plan 格式。**规范化工作共用这一份 plan**，总纲不另立 |

> 本文的「实现顺序」一节是**阶段划分的唯一真相源**。总纲故意不复制那张表，
> 修改阶段时只需改本文一处。
