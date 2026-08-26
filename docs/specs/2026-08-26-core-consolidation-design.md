# 核心下沉设计：让上层回归瘦客户端

**日期：** 2026-08-26
**状态：** 待实现
**范围：** `tyutool-core` / `tyutool-cli` / `src-tauri` / `tyutool-bridge` 的职责边界与契约收敛

---

## 背景与问题

AGENTS.md 已经写明「`tyutool-core` is the single source of truth for flash logic」。
这条约定在**烧录逻辑**上守住了——芯片插件、`run_job`、串口读写都在 core。

但在烧录逻辑之外，它没有守住。实测各 crate 代码量：

| crate | 行数 | 定位 | 实际 |
|---|---:|---|---|
| `tyutool-core` | 16578 | 唯一真相源 | plugins ~7000、`authorize.rs` 3335、`serial_debug.rs` 3288 |
| `tyutool-bridge` | 8798 | WS 助手 | 协议与安全为主，基本合理 |
| **`src-tauri`** | **6317** | **薄壳** | **不薄** |
| `tyutool-cli` | 2294 | 薄壳 | `reporter.rs` 562 是终端渲染，合理 |
| `tyutool-serve` | 2035 | 开发期 WS | — |

**上层四家合计 19444 行，超过 core 本身。** 一个真正的瘦客户端不该有 6317 行。

### 症状一：同一份逻辑写了三遍

```
crates/tyutool-bridge/src/main.rs:336   fn prune_log_files(dir: &Path)
crates/tyutool-cli/src/main.rs:361      fn prune_log_files(log_dir: &Path)
src-tauri/src/logs.rs:195               pub(crate) fn prune_log_files(log_dir: &Path)
```

`crates/tyutool-bridge/src/main.rs:66` 的注释自己写着
「Session log retention for this binary, **mirroring** `prune_log_files` in ...」——
这是被明确承认的复制。

AGENTS.md 里的日志治理约定（≤100 files / ≤100 MB）现在要靠三处实现同时保持一致。
改一次要记得改三处，漏一处不会报错。

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
| `crates/tyutool-core/src/job.rs:26` | `FlashJob`，17 个字段（唯一真相源） |
| `crates/tyutool-cli/src/main.rs:37` | clap `Commands` enum，Write/Read/Erase 各重复一遍 device/port/baud/start |
| `crates/tyutool-cli/src/main.rs:585,644,696` | **三处**手写 `FlashJob { .. }` 字面量，每处把用不上的字段显式写成 `None` |
| `src/features/firmware-flash/flash-ipc-types.ts` | 手工镜像 `FlashJob` + `FlashEvent`，100+ 行 |

`main.rs:585` 为了发一个 authorize job 写了 12 行 `None`。
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
| `redact_log_content` / `mask_value_range` / `write_logs_zip` / `build_report_info` | `src-tauri/src/logs.rs:883-978` | ~120 | 纯字符串 + zip；承载 `mask` 安全契约 |
| `is_newer` / `platform_key` / `verify_sha256` / `extract_binary_from_tar_gz` / `extract_binary_from_zip` | `crates/tyutool-cli/src/update.rs` | ~120 | 纯逻辑；`src-tauri/src/updater.rs`(524) 另有同类实现 |
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

### 1.3 待评估

`src-tauri/src/serial_debug.rs` 有 1025 行，而 `core/serial_debug.rs` 已有 3288 行，
`tyutool-serve` 中另有一套 WS 侧处理。三者的边界需要单独审计后再决定，
本设计不预判结论（见 P4）。

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
  ├─ updater.rs              ← 新增：版本比较 / 平台键 / SHA256 / 解包
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
default = []
excel = ["dep:calamine", "dep:rust_xlsxwriter"]
```

CLI 若要支持批量授权表，显式开启 `features = ["excel"]`；bridge 不开。

### 2.3 目标行数

| crate | 现在 | 目标 | 变化 |
|---|---:|---:|---|
| `src-tauri` | 6317 | ~2500 | Tauri 命令 + 事件桥接 + 编辑器探测 + window |
| `tyutool-cli` | 2294 | ~1800 | `reporter.rs` 是大头，不动 |
| `tyutool-bridge` | 8798 | ~7500 | 主体是协议与安全，本就该在自己家 |
| `tyutool-core` | 16578 | ~19500 | — |

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

> **风险：** `FlashPhase` / `FlashMilestone` 带 `#[serde(tag)]`，
> 现在手写成 `{ write_segment: { current, total } }` 这类形状。
> ts-rs 能按 serde 属性生成对应形状，但**需要实际验证**。
> 落地顺序：先打通纯 struct 的 `FlashJob`，再推枚举型的 `FlashEvent`。

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

**注意**：P3 完成前，批量操作没有等价 CLI 命令可回显，因为 CLI 尚无该能力。
命令回显在 P3 前只覆盖单设备场景。

---

## Section 4：明确不做的事

记录否决理由，避免后续反复讨论。

| 方案 | 否决理由 |
|---|---|
| **核心跑成守护进程，GUI 改瘦客户端** | `src-tauri/src/lib.rs:683` 的 40 个命令中，`logs::register_dialog_path`、`reset_main_window_layout`、`tauri-plugin-store`、文件对话框**无法搬入守护进程**，结果是命令表分裂成两半。串口是独占资源，多客户端并发是伪需求。真正的成本在安装 / 自启 / 升级 / 版本漂移 / 端口冲突 / 三套平台服务，是永久运维负担。`tyutool-bridge` 已覆盖「远程 web 客户端」这个唯一真实需求。参照 rclone：`rcd` 是可选模式，本体仍是直接执行的 CLI。 |
| **把 40 个 Tauri 命令重组成统一命令表** | `serial_debug_*` / `logs::*` / `updater::*` 本就是不同动作，强行统一是过度抽象（违反 AGENTS.md §2 Simplicity First）。 |
| **引入 `tauri-specta`** | v2 长期停留在 `2.0.0-rc.24`。生产项目不为省类型手工活押注 RC。`ts-rs` 只导出类型、不碰 IPC 层，风险低得多。 |
| **引入 TauRPC / rspc** | 要求把现有命令重组成 trait / router，改动面远超收益。 |
| **改动 `tyutool-serve` / `tyutool-bridge` 的协议** | 两者的消息枚举各自保留，只让 payload 类型指向同一批 core 结构。`validate_ws_origin` 按 AGENTS.md 要求「ported verbatim，do not rewrite」。 |

---

## Section 5：验收标准

1. `grep -c "FlashJob {" crates/tyutool-cli/src/main.rs` 结果为 `0`
2. `grep -rn "fn prune_log_files" crates src-tauri` 只剩一处，位于 `tyutool-core`
3. `cargo test -p tyutool-core` 产出 `src/bindings/*.ts`，且 `pnpm run build` 类型检查通过
4. `src/features/firmware-flash/flash-ipc-types.ts` 不再包含手写的 `FlashJobPayload`
5. 给 `FlashJob` 加一个新字段，**只改 `job.rs` 一处**，`cargo build` + `pnpm run build` 全绿
6. 真机跑一次烧录，日志中能找到 `to_cli_command()` 输出，复制出来可直接重放
7. `cargo build -p tyutool-cli` 不链接 `calamine` / `rust_xlsxwriter`（未开 `excel` feature 时）
8. AGENTS.md 中「Frontend types manually mirror」一条已删除

---

## 实现顺序

| 优先级 | 动作 | 量级 | 收益 |
|:---:|---|---|---|
| **P0** | `prune_log_files` 三合一 → `core/diagnostics.rs` | 半天 | 消掉已被承认的复制；验证下沉路径可行 |
| **P1** | updater 纯逻辑下沉 → `core/updater.rs` | 1–2 天 | 两套版本比较规则合一 |
| **P2** | `logs.rs` 纯逻辑层（~370 行）下沉 | 3–4 天 | **CLI 白捡日志列举 / tail / 导出 zip / 脱敏**；`mask` 安全契约变成单点实现 |
| **P2.5** | `FlashJob::Default` + `From<Args>` + `ts-rs` + `to_cli_command()` | 3–5 天 | 契约收敛；命令回显上线 |
| **P3** | 批量编排下沉 + `excel` feature | **2–3 周，有风险** | CLI 获得批量烧录 / 授权能力 |
| **P4** | `src-tauri/src/serial_debug.rs`(1025) 对照 `tyutool-serve` 审计 | 待评估 | — |

### P3 的风险须单列

2094 行**不是机械搬迁**。GUI 的 batch 深度依赖 Tauri 事件流做多口进度上报，
必须先把「不含 IO 的编排状态机」与「事件发射」拆开——这本质上是重写而非移动。
建议 P0–P2 完成、下沉路径经过验证后再评估，不要一开始就啃它。

P0–P2.5 合计约一周半，可将 `src-tauri` 从 6317 降至约 5000，
并留下一条被验证过、可重复的下沉路径。

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

## 后续文档

实现计划另立 `docs/plans/2026-08-26-core-consolidation.md`，按 P0–P4 拆成
checkbox 任务，遵循本仓库既有 plan 文档格式。
