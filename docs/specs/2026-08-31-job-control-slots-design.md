# 任务控制的两个共用槽位：`CancelSlot` 与 `ConfirmSlot`

**日期：** 2026-08-31
**状态：** 待实现
**范围：** `tyutool-core` 新增 `job_control` 模块；`src-tauri` 与 `tyutool-serve` 改为使用它
**计划：** `docs/plans/2026-08-31-job-control-slots.md`
**前置：** `docs/specs/completed/2026-08-31-flash-test-seams-design.md`（那轮给两边都补上了测试，
本次重构才有网可依）
**实测基准：** 行号实测于 `869fcb1`（`refactor/v3`）。

---

## 背景

`AGENTS.md` 写着：

> Never re-implement a helper that already exists in another crate. If two binaries
> need the same behaviour, it belongs in `tyutool-core`.

这条目前是被违反的状态：`src-tauri` 的 `flash_run` 和 `tyutool-serve` 的
`handle_connection` 各自实现了同一件事——**同一时刻只跑一个任务，且新任务不得抹掉旧任务的
取消**——而且实现方式不同。

**这不是理论问题，它已经造成过一个 bug。** 上一轮在 serve 里发现并修掉的取消失效
（PR #171，`d61d987`），根因就是两边对「任务与消息循环如何共存」各想各的。

### 现状对照（`869fcb1`）

| | `src-tauri`（`lib.rs:172-201`） | `tyutool-serve`（`lib.rs:455-483`） |
|---|---|---|
| 取消标志 | 互斥锁里**换一个新 Arc**，旧的留在 `true` | **复用同一个 Arc**，新任务把它重置为 `false` |
| 并发策略 | 等旧线程最多 3 秒，超时则拒绝新任务 | 直接拒绝第二个任务 |
| 并发模型 | `std::thread` + 阻塞 `join` | `tokio::spawn` + `await` |
| 事件出口 | `app.emit` | mpsc → WS sink |

**取消标志那一行是真正的问题所在。** src-tauri 的做法自带正确性——两个任务从不共用一个
标志位。serve 的做法之所以安全，是因为**另一个地方**（拒绝并发任务）恰好挡住了危险路径；
correctness 依赖于一个远处的约束，而不是这个数据结构自身的性质。这种安全是脆的：哪天
并发策略一放松，重置就会把前一个任务的取消一并抹掉，而且不会有任何东西报错。

另有一处**完全相同**的重复：确认握手。两边都是
`Arc<Mutex<Option<mpsc::Sender<bool>>>>`，都是「建 channel → 存 sender → 阻塞 recv」，
解析端都是「take sender → send」。逐行等价。

---

## 决策

### 不做什么

**不合并线程模型。** 一边是 `std::thread` + 阻塞 join，一边是 `tokio::spawn` + await；
一边往 `AppHandle` emit，一边往 mpsc 送。把这些塞进一个抽象，做出来的东西会比两份清楚的
重复更难懂——错误的抽象比重复更贵。**两个前端各自保留自己的线程模型和并发策略。**

**不动并发策略。** src-tauri 等 3 秒、serve 直接拒绝，这是两种前端的合理差异（GUI 用户会
连点按钮；WS 客户端不该并发发任务）。

### 做什么

在 `tyutool-core` 新增 `job_control` 模块，只提取两样**语义**，不提取控制流：

#### 1. `CancelSlot`

```rust
/// Hands out the cancel flag for one job at a time, so that starting a new job
/// can never clear the flag an old one is still watching.
pub struct CancelSlot { /* Mutex<Arc<AtomicBool>> */ }

impl CancelSlot {
    /// Signal whatever is running to stop, and hand back a *fresh* flag for the
    /// new job. The old Arc stays `true` forever.
    pub fn begin(&self) -> Arc<AtomicBool>;
    /// Signal the current job to stop.
    pub fn cancel(&self);
    /// The flag of the job already in flight.
    pub fn current(&self) -> Arc<AtomicBool>;
}
```

关键在于 `begin()` **返回新标志**而不是重置旧的——正确性变成这个类型自身的性质，不再依赖
调用方是否恰好禁止了并发。serve 由此从「安全但靠远处约束」变成「结构上就不可能错」。

#### 2. `ConfirmSlot`

```rust
/// The overwrite-confirmation handshake between a blocking job thread and
/// whatever frontend is going to ask the user.
pub struct ConfirmSlot { /* Mutex<Option<Sender<bool>>> */ }

impl ConfirmSlot {
    /// Blocks until answered. Called from the job thread.
    pub fn ask(&self) -> bool;
    /// Answer a pending ask; false if nothing was pending.
    pub fn resolve(&self, answer: bool) -> bool;
    /// Drop a stale pending ask left by a run that exited abnormally.
    pub fn clear(&self);
}
```

`resolve` 返回 bool 同时满足两边：src-tauri 的 `authorize_confirm_cmd` 靠它区分「有待决」
与「无待决」（后者要返回 Err，否则前端会以为对话框处理过了）；serve 忽略返回值。

**「弹什么、怎么弹」留在前端。** serve 在阻塞之前还要往 WS 送一帧 `AuthConflict`，那是它
的事，不进 core。

---

## 明确否掉的方案

**把 `run_job` 的整段编排搬进 core（含线程管理）。** 这是最初想到的做法，也是 spec
`2026-08-31-flash-test-seams-design.md` 里提过的「合并重复编排」。否掉的理由见上：线程模型
不同，硬合并会造出一个既不像线程也不像任务的四不像。

**照搬 `tyutool-bridge` 的 `FlashBackend`。** 那是一个「把整个执行面注入进来」的 trait，
解决的是**测试注入**问题，不是**共用不变量**问题。bridge 已经有它了，且 bridge 不在本次范围。

**统一并发策略。** 见上，两种前端的差异是合理的。

---

## 验收

- 两个前端的**外部行为一律不变**：serve 仍拒绝并发任务，src-tauri 仍等 3 秒后拒绝。
- 上一轮建立的测试是这次重构的网：serve 31 个、src-tauri 36 个，重构前后都必须全绿。
- `CancelSlot` 自身要有测试，尤其是那条不变量：**`begin()` 之后，旧标志仍为 `true`**。
