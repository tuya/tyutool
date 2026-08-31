# 烧录编排可测缝隙 实施计划

**Spec:** `docs/specs/completed/2026-08-31-flash-test-seams-design.md`

**状态：** 已交付——PR #171，rebase-merge 至 `refactor/v3`，末位提交 `869fcb1`。
CI 全绿（core+cli+serve 1m41s、tauri 2m8s、bridge 2m26s、frontend 51s）。

**Goal:** 给 `tyutool-core` 开一个插件注入缝隙，并提供一个 feature 门控的假芯片插件，
使 `run_job` 的编排逻辑（事件顺序、取消、错误映射）第一次可以在没有硬件的情况下被测试。

**Architecture:** 三处加法，零处改造。`FlashPluginRegistry` 加 `register()`；`run_job`
拆出 `run_job_with(registry, ..)` 并把自己实现成它的一层薄封装；假插件放在
`plugins/mock.rs`，由新的 `mock-chip` feature 门控。现有三个前端的调用方式完全不变，
产线代码路径不动。形状照抄 `tyutool-bridge` 的 `FlashBackend`（仅参照，不修改 bridge）。

**Tech Stack:** Rust 2021（rustc 1.98）。测试是 `tyutool-core` 内联的
`#[cfg(test)] mod tests`。CI 门禁见 `AGENTS.md` 的 Commands 一节。

**基线（`edece84`，全绿）：** core 313 / cli 59 / serve 28 / tauri 32 / 前端 62 个文件。

---

## File Map

| File | Change |
|---|---|
| `crates/tyutool-core/Cargo.toml` | 新增 `mock-chip` feature（带「谁该开、谁不该开」的注释） |
| `crates/tyutool-core/src/registry.rs` | 加 `FlashPluginRegistry::register()`；拆出 `run_job_with()`；新增编排测试 |
| `crates/tyutool-core/src/plugins/mock.rs` | 新文件：`MockPlugin`，feature 门控 |
| `crates/tyutool-core/src/plugins/mod.rs` | 挂 `mock` 模块并 re-export，feature 门控 |
| `crates/tyutool-core/src/lib.rs` | re-export `run_job_with`，以及 feature 门控的 `MockPlugin` |
| `.github/workflows/ci.yml` | 把 `mock-chip` 加进 optional-features 那一步 |
| `AGENTS.md` | 同步 Commands 里的 CI 门禁命令行 |

---

## Task 1: 新增 `mock-chip` feature

**Files:** Modify `crates/tyutool-core/Cargo.toml`

- [x] **Step 1:** 在 `[features]` 段末尾追加，注释遵循既有 feature 的体例（说明谁必须开、谁必须不开）:

```toml
# Enables the scripted mock chip plugin (`plugins::mock::MockPlugin`), which
# answers a flash job without touching a serial port. Test-only: every shipped
# binary must leave it off — it is enabled by `cargo test` invocations and
# nothing else. It lives behind a feature rather than `#[cfg(test)]` because
# `#[cfg(test)]` items exist only while compiling *this* crate's own tests, and
# tyutool-serve / src-tauri tests are separate compilation units that could not
# see it.
mock-chip = []
```

- [x] **Step 2:** 验证 `cargo check -p tyutool-core --features mock-chip` 通过（此时还没有代码，应当仍然通过）

---

## Task 2: `FlashPluginRegistry::register`

**Files:** Modify `crates/tyutool-core/src/registry.rs`

- [x] **Step 1:** 在 `impl FlashPluginRegistry` 里，`new()` 之后加：

```rust
/// Register a plugin under its own [`FlashPlugin::id`], replacing any plugin
/// already held under that id.
///
/// The key goes through [`normalize_chip_id`] so registration and
/// [`get`](Self::get) agree on the same spelling. Replacement is deliberate:
/// a test can swap a real chip id (`T5AI`) for a scripted stand-in and so
/// exercise the path a frontend actually takes, rather than a made-up id no
/// caller would ever send.
pub fn register(&mut self, plugin: Arc<dyn FlashPlugin>) {
    let key = normalize_chip_id(plugin.id());
    log::debug!("Registered flash plugin: {}", key);
    self.plugins.insert(key, plugin);
}
```

- [x] **Step 2:** `cargo test -p tyutool-core` 仍然全绿（313 passed）

---

## Task 3: 拆出 `run_job_with`

**Files:** Modify `crates/tyutool-core/src/registry.rs`, `crates/tyutool-core/src/lib.rs`

- [x] **Step 1:** 把现有 `run_job` 的函数体整体移进新的 `run_job_with`，签名多一个
  `registry: &FlashPluginRegistry` 首参；函数体内 `let reg = default_registry();`
  改成用传进来的 `registry`。**其余一行不改**（日志、事件、错误映射全部原样）。

- [x] **Step 2:** `run_job` 改写成：

```rust
/// Run a job against the default registry (CLI, serve and Tauri all use this).
pub fn run_job<F>(job: &FlashJob, cancel: &AtomicBool, progress: F) -> Result<(), FlashError>
where
    F: Fn(FlashEvent),
{
    run_job_with(default_registry(), job, cancel, progress)
}
```

- [x] **Step 3:** `lib.rs` 的 `pub use registry::{...}` 里加上 `run_job_with`

- [x] **Step 4:** `cargo test -p tyutool-core -p tyutool-cli -p tyutool-serve` 全绿；
  `cargo clippy -p tyutool-core --all-targets -- -D warnings` 干净

---

## Task 4: `MockPlugin`

**Files:** Create `crates/tyutool-core/src/plugins/mock.rs`; modify `plugins/mod.rs`, `lib.rs`

- [x] **Step 1:** 新建 `plugins/mock.rs`。文件头注释必须写明**它买不到什么**（不发一个
  字节到串口，因此不提供任何协议保障）。提供：

  - `MockPlugin::with(id, closure)` — 全能形式，闭包签名与 `FlashPlugin::run` 一致
  - `MockPlugin::ok(id)` — 走一遍 Handshake → Percent(100) 后成功
  - `MockPlugin::failing(id, msg)` — 立即返回 `FlashError::Plugin(msg)`
  - `MockPlugin::blocking_until_cancelled(id)` — 轮询 `cancel`，置位后返回 `FlashError::Cancelled`

- [x] **Step 2:** `plugins/mod.rs` 加 feature 门控的 `pub mod mock;` 与 re-export

- [x] **Step 3:** `lib.rs` 加 feature 门控的 `pub use plugins::mock::MockPlugin;`

- [x] **Step 4:** `cargo clippy -p tyutool-core --all-targets --features mock-chip -- -D warnings` 干净

---

## Task 5: 编排测试

**Files:** Modify `crates/tyutool-core/src/registry.rs`（`mod tests`）

- [x] **Step 1:** 加一组 `#[cfg(feature = "mock-chip")]` 的测试，覆盖今天完全空白的部分：

  - 成功路径的**事件顺序**：首个事件是 `JobSummary`，末个是 `Done{Ok}`
  - 插件报错 → 返回 `Err` **且**发出 `Done{Err{message}}`，message 是插件原文
  - 取消 → 发出 `Done{Cancelled}`，且 `run_job_with` 返回 `Err(Cancelled)`
  - `register` 覆盖已有 id：注册到 `T5AI` 的假插件确实被调用
  - 芯片 id 归一化：`chip_id: "t5"` 能路由到注册在 `T5AI` 上的假插件

- [x] **Step 2:** `cargo test -p tyutool-core --features mock-chip` 全绿，且新测试数 > 0

---

## Task 6: CI 与文档

**Files:** Modify `.github/workflows/ci.yml`, `AGENTS.md`

- [x] **Step 1:** `ci.yml` 的 "Clippy and test tyutool-core's optional features" 一步，
  把 `--features zip,excel` 改成 `--features zip,excel,mock-chip`，并在该步已有的注释里
  补一句说明。**不加这一步，新测试在 CI 里等于不存在**（`AGENTS.md` 已有此规则）。

- [x] **Step 2:** 同步 `AGENTS.md` Commands 一节里那两行门禁命令

- [x] **Step 3:** 跑完整门禁：

```bash
cargo fmt --all --check
cargo clippy -p tyutool-core -p tyutool-cli -p tyutool-serve --all-targets -- -D warnings
cargo clippy -p tyutool-core --all-targets --features zip,excel,mock-chip -- -D warnings
cargo test  -p tyutool-core -p tyutool-cli -p tyutool-serve
cargo test  -p tyutool-core --features zip,excel,mock-chip
cargo test  -p tyutool_gui
```

---

## 验收标准

- 上面全部门禁通过，且基线计数只增不减（core ≥ 313 + 新增，其余不变）
- `cargo test -p tyutool-core`（不带 feature）仍然全绿——假插件不得泄漏进默认编译
- 产线代码路径零改动：`run_job` 的三个调用方（cli / serve / src-tauri）一行未动

---

# Phase 1.5：把假设备铺到所有前端

**动机：** `run_job_with` 只有 core 自己的测试用得上。serve 的 `handle_run_job` 和
src-tauri 的 `flash_run` 都直接调 `run_job`，要让它们也能跑假芯片，只需要把 `MOCK` 注册
进**默认**注册表——三个前端一个签名都不用改。代价是假芯片进了默认表，因此必须同时上锁。

## Task 7: `MockPlugin::simulated`

**Files:** Modify `crates/tyutool-core/src/plugins/mock.rs`

- [x] **Step 1:** 加 `simulated(id)`：按真实时间走完 handshake → flash id → erase →
  `Percent` 爬坡 → verify → reboot，全程检查取消标志。故意耗时（约 1.5 秒，每 50 ms 一
  步）——瞬时返回的任务无法被中途取消，而那正是要验证的行为。

## Task 8: `MOCK` 进默认注册表

**Files:** Modify `crates/tyutool-core/src/registry.rs`

- [x] **Step 1:** 加 `pub const MOCK_CHIP_ID: &str = "MOCK"`（feature 门控）
- [x] **Step 2:** `FlashPluginRegistry::new()` 末尾按 feature 注册 `MockPlugin::simulated`
- [x] **Step 3:** 修 `list_chip_ids_only_real_plugins`——它硬断言 `len() == 11`，开着
  feature 会变 12。改成按 `cfg!(feature = ...)` 取期望值，并保留「多一个都不行」的语义

## Task 9: 两道封锁

**Files:** Modify `crates/tyutool-core/src/lib.rs`, `Cargo.toml`;
Create `crates/tyutool-core/tests/shipped_crates_exclude_mock_chip.rs`

- [x] **Step 1:** `lib.rs` 顶部加
  `#[cfg(all(feature = "mock-chip", not(debug_assertions)))] compile_error!(..)`，
  注释写明为什么 `debug_assertions` 是可靠信号、代价是什么、将来要换不要删
- [x] **Step 2:** 新建守卫测试，结构化遍历四个出包 crate 的 manifest，两种写法都盖
  （`features = ["mock-chip"]` 与 `"tyutool-core/mock-chip"` 转发）。**不加 feature 门控**
- [x] **Step 3:** `toml = "0.8"` 进 dev-dependencies（已在 workspace lock 里，不引入新版本）
- [x] **Step 4:** 更新 `Cargo.toml` 里 feature 的注释，写明两道保障

## Task 10: 阴性验证

- [x] **Step 1:** `cargo build --release -p tyutool-core --features mock-chip` → **必须编译失败**
- [x] **Step 2:** `cargo build --release -p tyutool-core` → 必须成功
- [x] **Step 3:** 临时给 `crates/tyutool-cli/Cargo.toml` 加 `"mock-chip"` → 守卫测试必须失败；还原
- [x] **Step 4:** 临时给 `crates/tyutool-serve/Cargo.toml` 加转发项 → 守卫测试必须失败；还原

## Phase 1.5 验收标准

- 两道封锁都经过**阴性验证**——只确认「正常情况下测试通过」是不够的，必须确认它们
  在该拦的时候真的拦得住
- `cargo test -p tyutool-core`（不带 feature）仍然全绿，且守卫测试在这一次里也跑到了
- 产线代码路径依旧零改动

---

# Phase 2：用假设备验 `tyutool-serve` 的消息循环

**前提：** 本阶段是本次工作里**第一个会改产线代码的**阶段。顺序必须是「先立住复现
测试，看它红，再改代码，看它绿」——先改后补测试无法证明测试真的能抓到这个 bug。

## Task 11: 给 serve 接上假设备

**Files:** Modify `crates/tyutool-serve/Cargo.toml`,
`crates/tyutool-core/tests/shipped_crates_exclude_mock_chip.rs`

- [x] **Step 1:** serve 加 `[dev-dependencies] tyutool-core = { features = ["mock-chip"] }`。
  不能用普通依赖（守卫禁止），也不能用 `#[cfg(feature)]`（serve 未声明该 feature）
- [x] **Step 2:** 守卫测试排除 `dev-dependencies` 表（任意嵌套层级），并写明依据：
  `cargo build --release` 根本不编译 dev-dependency
- [x] **Step 3:** 验证例外安全：`cargo build --release -p tyutool-cli`（它依赖 serve）
  必须成功——若 feature 泄漏，第一道封锁会当场炸

## Task 12: 复现测试（先看它红）

**Files:** Modify `crates/tyutool-serve/src/lib.rs`

- [x] **Step 1:** 在 `mod tests` 下新增 `mod loop_responsiveness`，起真实 `run_serve`、
  用真实 WS 客户端连接、跑 `chipId: "MOCK"` 的任务
- [x] **Step 2:** 基线用例：一个 MOCK 任务能通过 WS 完整跑完（应当直接通过）
- [x] **Step 3:** 复现用例：任务跑到一半发 `cancel`，断言收到 `Done{Cancelled}`
  → **实测失败**，任务照样跑完 1.505 秒后报 `ok`。bug 坐实

## Task 13: 修复（再看它绿）

**Files:** Modify `crates/tyutool-serve/src/lib.rs`

- [x] **Step 1:** `handle_run_job` 改为 `tokio::spawn`，不再在循环体里 `.await`；
  签名改成按值接收 `sink_tx` 与 `job`
- [x] **Step 2:** 新增「一条连接同时只一个任务」的拒绝。以前靠循环阻塞隐含保证，
  现在不拒的话，第二个任务会把第一个的取消标志清掉
- [x] **Step 3:** 连接关闭时 `await` 任务句柄，再丢 sink
- [x] **Step 4:** 复现用例转绿；补一个拒绝并发任务的用例（新行为必须有证据）
- [x] **Step 5:** 核对前端：`ws-transport.ts` 的 runJob 路径已处理 `type === "error"`，
  不需改动

## Phase 2 验收标准

- 复现测试在修复前**确实失败过**，修复后通过；先绿的测试不算复现
- serve 套件 28 → 31 全绿
- 假设备仍然进不了发布产物（例外验过）

---

# Phase 3：用无头 mock 运行时测 `src-tauri` 的 flash 命令

**动机：** 这是**发给用户的那条路径**。`flash_run` 里取消 Arc 的原子替换、等旧线程
3 秒、confirm 通道，是仓库里最难看懂的几十行，而 979 行的 `lib.rs` 只有 32 个测试。

## Task 14: dev-dependencies

**Files:** Modify `src-tauri/Cargo.toml`

- [x] **Step 1:** 加 `tauri = { version = "2", features = ["test"] }`（无头 mock 运行时）
- [x] **Step 2:** 加 `tyutool-core = { features = ["mock-chip"] }`，同样走 dev-dependency
  ——`tauri build` 底下是 `cargo build --release`，不编译 dev-dependency

## Task 15: `flash_run` 泛型化（唯一的产线修改）

**Files:** Modify `src-tauri/src/lib.rs`

- [x] **Step 1:** `fn flash_run<R: Runtime>(app: AppHandle<R>, ..)`。裸写的 `AppHandle` 带
  `#[default_runtime]`，实际是 `AppHandle<Wry>`，需要真实 WebView，测试里造不出来。
  只改签名，行为不变；`generate_handler!` 依旧解析到 app 自己的运行时

## Task 16: 测试

**Files:** Modify `src-tauri/src/lib.rs`

- [x] **Step 1:** 在**主** `mod tests` 下新增 `mod flash_commands`，用
  `mock_builder` + `mock_context(noop_assets())` 造 app，`manage` 两个 state
- [x] **Step 2:** 四个用例：任务跑完且进度以 `flash-progress` 送达；中途 `flash_cancel`
  生效；启动第二个任务把第一个取消（验证那段注释声称的性质）；无待决确认时
  `authorize_confirm_cmd` 报错
- [x] **Step 3:** ⚠ 落点必须是主 `mod tests`。测试最初落在了 `mod xdg_command_tests`，
  而它带 `#[cfg(target_os = "linux")]`——ubuntu CI 上照跑，但 Windows / macOS 上会静默
  消失。已移回

## Task 17: 封锁验证

- [x] **Step 1:** 守卫测试仍通过（dev-dependencies 是已写明依据的例外）
- [x] **Step 2:** `cargo build --release -p tyutool_gui` 必须成功——假芯片若泄进 GUI 产物，
  第一道封锁会当场编译失败

## Phase 3 验收标准

- src-tauri 套件 32 → 36 全绿，且新用例不在任何 `cfg` 门控的模块里
- 产线改动仅限 `flash_run` 的签名泛型化，无行为变化
- GUI 的 release 构建干净

---

# Phase 4：录制回放，给协议一条真实基线

**前提：** 需要一次真实硬件。本次用的是 T5AI 开发板（CH342，A 口 COM34）。

## Task 18: `RecordIo` / `ReplayIo`

**Files:** Modify `crates/tyutool-core/src/plugins/beken/transport.rs`, `Cargo.toml`,
`crates/tyutool-core/src/plugins/beken/driver.rs`

- [x] **Step 1:** 线格式 `TraceOp`：每行一个操作的十六进制文本（不用 JSON/base64，不引
  新依赖，而且 fixture 的 diff 能看）。编解码共一份，`cfg(any(feature, test))`
- [x] **Step 2:** `RecordIo<T>` 包住活 transport，追写到 `TYUTOOL_RECORD_IO`；`record-io` feature
- [x] **Step 3:** `ReplayIo`（`#[cfg(test)]`）回放并校验
- [x] **Step 4:** `run_beken` 里接上 env 分支（`Transport` 本就泛型，包一层即可）
- [x] **Step 5:** `record-io` 加进守卫测试的 `TEST_ONLY_FEATURES`，并把守卫泛化为按列表检查

## Task 19: 实机录制

- [x] **Step 1:** 先做**无损**探测（读 4 KiB）确认链路通，再碰写操作
- [x] **Step 2:** 带 `--features tyutool-core/record-io` 构建 Windows CLI（COM34 在 Windows 侧）
- [x] **Step 3:** 录制 `read -s 0x0 -l 0x1000` → 163 个操作 / 9021 字节
- [x] **Step 4:** 烧录 `auth-firmware-t5ai-1.1.1.bin`（2.36 MiB / 50.2s），回读前 4 KiB 与固件
  逐字节比对一致
- [x] **Step 5:** ⚠ 烧录**之后重录**一次。首次录制里的 4 KiB 是旧固件内容，内容不明；
  重录后 fixture 里就是那份公开固件的头部

## Task 20: 回放测试

- [x] **Step 1:** fixture 放 `plugins/beken/t5ai-read-4k.trace`（比照 `ln882h/ram.bin` 的位置），
  头部写明出处、设备、命令、不要手改
- [x] **Step 2:** 回放测试驱动 `run_beken_on_transport`，断言读出 4 KiB 且
  `remaining() == 0`
- [x] **Step 3:** 首次回放在 op 14 分叉。**不是 bug**：`recv_frame` 由真实时间截止，回放时
  读取瞬返回、循环多转了几圈。改为「下一个录制操作不是读取时返回超时且不消耗操作」
  ——**读取宽松，写入严格**
- [x] **Step 4:** ⚠ **金标准必须阴性验证**。改 fixture 里一条写入的一个字节 → 测试如期失败
  并指出 op 位置与两边字节串；已还原

## Task 21: CI 与文档

- [x] **Step 1:** `record-io` 加进 `ci.yml` 的 optional-features 一步（它自己没测试，
  放进去是为了不让它默默腐掉），同步 `AGENTS.md`

## Phase 4 验收标准

- 回放测试跑在**普通** `cargo test -p tyutool-core` 里（`#[cfg(test)]`，不要 feature）
- 金标准经过阴性验证
- fixture 头部说清楚出处，且其中设备内容不含秘密
- 真实烧录路径实机验证无回归
