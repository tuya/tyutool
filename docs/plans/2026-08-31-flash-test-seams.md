# 烧录编排可测缝隙 实施计划

**Spec:** `docs/specs/2026-08-31-flash-test-seams-design.md`

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
