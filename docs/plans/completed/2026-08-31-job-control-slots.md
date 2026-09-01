# 任务控制槽位 实施计划

**Spec:** `docs/specs/completed/2026-08-31-job-control-slots-design.md`

**状态：** 已交付——PR #173，rebase-merge 至 `refactor/v3`，提交 `36997e0`。

**Goal:** 把「同一时刻只跑一个任务，且新任务不得抹掉旧任务的取消」这个不变量，以及确认握手，
从 `src-tauri` 与 `tyutool-serve` 各自的实现中提取到 `tyutool-core`，让正确性成为数据结构
自身的性质而不是远处约束的副产品。

**Architecture:** core 新增一个 flat 模块 `job_control.rs`，放 `CancelSlot` 与 `ConfirmSlot`。
两个前端替换掉自己那份，**保留各自的线程模型与并发策略**。外部行为零变化。

**Tech Stack:** Rust 2021。core 的测试内联；两个前端的行为由上一轮建立的测试兜底
（serve 31、src-tauri 36），重构前后都必须全绿。

**基线（`869fcb1`）：** core 322 / cli 59 / serve 31 / tauri 36。

---

## File Map

| File | Change |
|---|---|
| `crates/tyutool-core/src/job_control.rs` | 新文件：`CancelSlot`、`ConfirmSlot` + 测试 |
| `crates/tyutool-core/src/lib.rs` | 挂模块并 re-export |
| `src-tauri/src/lib.rs` | `FlashState.cancel` / `ConfirmState` 改用槽位；`flash_run`、`flash_cancel`、`authorize_confirm_cmd` 跟着改 |
| `crates/tyutool-serve/src/lib.rs` | 连接级 `cancel` / `pending_confirm` 改用槽位 |
| `docs/specs/…-design.md` / `docs/plans/…` | 本对文档 |

---

## Task 1: `CancelSlot`

**Files:** Create `crates/tyutool-core/src/job_control.rs`; modify `lib.rs`

- [x] **Step 1:** `begin()` 在互斥锁里 `std::mem::replace` 出新 Arc，并把旧的置 `true`；
  `cancel()`、`current()`
- [x] **Step 2:** 测试，重点是那条不变量：**`begin()` 之后旧标志仍为 `true`**，
  新标志为 `false`，两者不是同一个 Arc
- [x] **Step 3:** `cargo test -p tyutool-core` 仍 322 + 新增

## Task 2: `ConfirmSlot`

**Files:** Modify `crates/tyutool-core/src/job_control.rs`

- [x] **Step 1:** `ask()` / `resolve()` / `clear()`。`resolve` 返回「是否真的有待决」
- [x] **Step 2:** 测试：无待决时 `resolve` 返回 false；`ask` 在另一线程 `resolve` 后返回；
  `clear` 之后原来的 `ask` 得到 false 而不是永久阻塞

## Task 3: `src-tauri` 改用槽位

**Files:** Modify `src-tauri/src/lib.rs`

- [x] **Step 1:** `FlashState.cancel` 换成 `CancelSlot`；`ConfirmState` 换成 `ConfirmSlot`
- [x] **Step 2:** `flash_run` 里那段「换新 Arc + 置旧为 true」换成 `slot.begin()`，
  **3 秒 join 与拒绝逻辑原样保留**
- [x] **Step 3:** `flash_cancel` → `cancel()` + `resolve(false)`；
  `authorize_confirm_cmd` → 用 `resolve()` 的返回值决定 Ok/Err
- [x] **Step 4:** `cargo test -p tyutool_gui` 仍 **36 全绿**（行为不变的证据）

## Task 4: `tyutool-serve` 改用槽位

**Files:** Modify `crates/tyutool-serve/src/lib.rs`

- [x] **Step 1:** 连接级 `cancel` 换成 `CancelSlot`。⚠ 原来的 `cancel.store(false)`
  换成 `begin()`——这是本次的**实质改进**：不再依赖「拒绝并发任务」来保证正确性
- [x] **Step 2:** `pending_confirm` 换成 `ConfirmSlot`；`AuthConflict` 帧仍在前端侧发
- [x] **Step 3:** 关闭连接时的 `cancel` + 唤醒改用槽位
- [x] **Step 4:** `cargo test -p tyutool-serve` 仍 **31 全绿**

## Task 5: 门禁

- [x] **Step 1:** 全量门禁（fmt / clippy ×3 / 全部测试 / bindings）
- [x] **Step 2:** 计数核对：core 增加，cli / serve / tauri **数量不变**——
  数量变了就说明行为动了，那是本次不该发生的事

---

## 验收标准

- 两个前端的测试数**一个不多一个不少**且全绿：行为不变的最好证据
- `CancelSlot` 有自己的不变量测试
- 线程模型、并发策略、事件出口**一行未动**
- `AGENTS.md` 的「never re-implement a helper」不再被违反
