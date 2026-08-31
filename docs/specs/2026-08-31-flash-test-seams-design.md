# 烧录编排的可测缝隙设计：假芯片插件

**日期：** 2026-08-31
**状态：** 待实现
**范围：** `tyutool-core` 的插件注册与 `run_job` 调度入口
**计划：** `docs/plans/2026-08-31-flash-test-seams.md`
**实测基准：** 本文所有计数、行号均实测于 `edece84`（`refactor/v3`）。修改本文时请重测并更新此行的 commit。

---

## 背景与问题

瘦客户端重构把烧录逻辑收敛进了 `tyutool-core`，三个前端（CLI、`tyutool-serve`、
`src-tauri`）都通过 `tyutool_core::run_job` 这一个入口调用它。**收敛做成了，但这个
入口本身没有测试。**

原因很单纯：`run_job` 走 `default_registry()` 取插件，而那是一个 `OnceLock` 全局
单例，`FlashPluginRegistry::new()` 里硬编码 11 个真实芯片，`plugins` 字段私有。
**外部没有任何办法塞一个假芯片进去。**

结果是 `registry.rs` 现有的 7 个测试只能做两件事：查表（`normalize_chip_id`、
`list_chip_ids`），以及拿一个不存在的端口去撞错误路径。`run_job` 真正负责的那部分
——事件顺序、取消语义、错误到 `Done` 的映射——**一行都没覆盖**。

实测基线（`edece84`，全绿）：

| 套件 | 通过数 |
|---|---:|
| `tyutool-core` | 313 |
| `tyutool-cli` | 59 |
| `tyutool-serve` | 28 |
| `src-tauri` | 32 |
| 前端 vitest | 62 个文件 |

数字不小，但**没有一个测试驱动过一次完整的 `run_job`**。

### 这不是新问题，仓库里已经有答案

`tyutool-bridge` 遇到过同一个问题并解决了。`crates/tyutool-bridge/src/lib.rs:260`：

```rust
/// Injectable flash execution surface: production wires tyutool-core
/// (`run_job` / `check_port_available`); tests inject fakes so job
/// orchestration and port arbitration run without real hardware.
pub trait FlashBackend: Send + Sync { ... }
```

生产用 `RealFlashBackend`，测试用 5 个不同的 `FakeBackend`；`run()` →
`run_with_enumerator()` → `run_with(enumerator, poll_interval, backend)` 逐层多暴露
一个可替换的东西。靠这套缝隙，bridge 有 7 个集成测试文件、约 145 个测试函数，全程
走真实 WebSocket，全程不需要硬件。

**本设计不发明新东西，只是把这个已经跑通的形状搬到 `tyutool-core`。**
bridge 本身不在本次范围内，仅作参照。

---

## 决策

### 1. `FlashPluginRegistry::register` — 注册（或覆盖）一个插件

```rust
pub fn register(&mut self, plugin: Arc<dyn FlashPlugin>)
```

键取 `normalize_chip_id(plugin.id())`，与 `get()` 的查表口径一致。允许**覆盖**已有
的芯片 id，这样测试可以把 `T5AI` 换成假的，验证「前端选了 T5AI 之后这条路接对了」，
而不只是验证一个虚构的 id。

`FlashPlugin` trait 本身已经 `pub use`（`lib.rs:44`），外部本就能实现，缺的只是注册。

### 2. `run_job_with` — 指定 registry 的 `run_job`

```rust
pub fn run_job_with<F>(registry: &FlashPluginRegistry, job: &FlashJob, cancel: &AtomicBool, progress: F)
    -> Result<(), FlashError>
```

`run_job` 退化成 `run_job_with(default_registry(), ..)`。编排逻辑只有一份，现有三个
前端的调用方式完全不变。

### 3. `mock-chip` feature — 假芯片插件

假插件放在 `crates/tyutool-core/src/plugins/mock.rs`，由 `mock-chip` feature 门控。

**为什么是 Cargo feature 而不是 `#[cfg(test)]`：** 现有的 `MockIo`
（`plugins/beken/transport.rs:471`）用的是 `#[cfg(test)]`，它只在编译 core 自己的单
测时存在。`tyutool-serve` 和 `src-tauri` 的测试是**另一个 crate 的编译单元**，看不见
它。要让上层前端也能用同一个假芯片，只能走 feature。

行为通过一个闭包注入，而不是自定义脚本 DSL——`FlashPlugin::run` 的签名本身就是那个
闭包，再包一层只会多一层翻译：

```rust
MockPlugin::with(id, |job, cancel, progress| { ... })   // 全能形式
MockPlugin::ok(id)                    // 发几个事件后成功
MockPlugin::failing(id, msg)          // 立即失败
MockPlugin::blocking_until_cancelled(id)  // 转圈等取消
MockPlugin::simulated(id)             // 按真实时间走完一次烧录，全程检查取消
```

### 4. `MOCK` 进默认注册表

开着 `mock-chip` 时，`FlashPluginRegistry::new()` 额外注册一颗 id 为 `MOCK` 的假芯片
（`MockPlugin::simulated`）。

这一条看似微小，却是整套东西能不能用到上层的关键。`run_job_with` 只能服务于 core 自己
的测试：`tyutool-serve` 的 `handle_run_job` 和 `src-tauri` 的 `flash_run` 都直接调 `run_job`，
想让它们用假芯片，要么给两个 crate 各自凿一个注入口（改两处产线函数签名），要么就是
这一行。选后者：**三个前端一个签名都不用改**，发一个 `chipId: "MOCK"` 的任务就行。

附带收益：开发前端时手上没板子也能真的把 GUI 跑起来——选 MOCK，进度条会走，取消会生效。

`simulated` 故意要花时间（约 1.5 秒，每 50 ms 看一次取消标志）。一个瞬时返回的任务根本无法被
中途取消，而「用户还能不能取消」正是这套假件要验证的东西。

---

## 封锁：`mock-chip` 绝不能进发布产物

把假芯片放进**默认**注册表，意味着一个误带该 feature 的构建会给用户一颗「能选、会走进度条、
但什么也没烧」的芯片。这不能靠约定，必须有硬保障。**两道，缺一不可：**

| | 拦什么 | 实现 |
|---|---|---|
| 一 | 命令行上的 `--features` 流进 release 构建 | `src/lib.rs` 顶部 `#[cfg(all(feature = "mock-chip", not(debug_assertions)))] compile_error!` |
| 二 | 某个出包 crate 把它写进了 manifest | `tests/shipped_crates_exclude_mock_chip.rs` |

**为什么 `debug_assertions` 是可靠信号：**三条发布路径全是 release ——
`cargo build --release -p tyutool-cli --target <triple>`（release.yml:132）、
`npx @tauri-apps/cli build`（release.yml:277）、
`cargo build --release -p tyutool-bridge`（bridge.yml:248）——而仓库里没有任何地方覆盖
`[profile.release].debug-assertions`。代价是 `cargo test --release --features mock-chip` 编不过；
目前没有任何 workflow 或 script 这么跑。将来若真需要，**换一个更窄的信号，不要直接删掉**。

第二道结构化遍历整份 manifest（不是扫文本，所以注释里提到 feature 名不会误报），两种写法都盖：
`tyutool-core = { features = ["mock-chip"] }` 和 `[features]` 里的转发形式
`"tyutool-core/mock-chip"`。它**不带** feature 门控，跑在普通的 `cargo test -p tyutool-core` 里，
也就是每次 push 都会跑的那一步。形状照搬 `crates/tyutool-bridge/tests/build_config.rs`（它用同
样的办法守 `+crt-static`）。

两道都做过阴性验证：release 带 feature 确实编译失败；往 cli 的 manifest 里加 `mock-chip`、
往 serve 的 manifest 里加转发项，守卫测试都如期报错。

---

## 明确否掉的方案

**PTY / 虚拟串口（socat、com0com、`virtual-serialport`）。** 这是业界常见做法，能覆盖
包括 ESP 在内的全部芯片，因为它在操作系统层面伪造串口，被测代码毫无察觉。否掉的理由
是跨平台：Windows 侧需要 com0com，要装驱动、要管理员权限，CI 上跑不了，而 Windows 正是
用户主力平台；Linux 侧 socat 的 pty 不支持全部 ioctl，而 tyutool 重度依赖 DTR/RTS 做
复位（`serial.rs:532`）。

**ESP 系列的协议级测试。** `plugins/esp/common.rs:234` 用 `open_native()` 拿到操作系统
真实句柄后整个交给 `espflash::Connection::new`，**塞不进任何假对象**。除了上面那条被否
掉的 PTY 路线，没有别的入口。放弃，不是疏忽。

**`mockall`。** 现有手写的 `MockIo` 已经够用，引入它多一个依赖和一层宏，收益不明显。

**为「可测」而重构 `flash_run`。** Tauri 2.11.5 自带 `test` feature 和
`src/test/mock_runtime.rs`，`flash_run` 原地就能测，不需要为了测试去动产线代码路径。
`src-tauri` 与 `tyutool-serve` 之间那份重复的编排逻辑仍然值得合并，但那是独立的技术债，
不是本设计的前置条件。

---

## 本设计能买到什么，买不到什么

**能买到：** 接线正确性。事件顺序、取消是否真的生效、底层错误有没有变成 `Done{Err}`
送到前端、授权确认回调有没有接上——这些是瘦客户端重构真正会碰坏的东西。

**买不到：** 任何协议正确性。假芯片不发一个字节到串口。覆盖率数字会因此上升，但烧录
协议本身的保障**一点没变**。这一句必须留在测试文件的头部注释里，否则以后看到覆盖率会
产生错误的安全感。

协议那一层另有方案（在现有 `IoTransport` 上加录制回放），不在本次范围。

---

## 后续阶段（不在本次范围）

| 阶段 | 内容 | 状态 |
|---|---|---|
| 二 | 测 `tyutool-serve` 的连接循环 | ✅ 已完成，见下 |
| 三 | 用 `tauri::test::mock_builder` 测 `flash_run` | ✅ 已完成，见下 |
| 四 | `ReplayIo`：录制真实串口字节流，回放做协议基线 | 待做；需要一次真实硬件录制 |

---

## 阶段二的结果：`tyutool-serve` 的取消失效（已坐实并修复）

假设备铺好后，第一件用它验的事就报了错。

**症状：** `handle_connection` 的消息循环是单任务顺序的，而 `handle_run_job(..)` 是在循环体里
`.await` 的。任务一旦跑起来，这条连接就**再也读不到客户端消息**——而 `Cancel` 和
`AuthorizeConfirm` 恰恰是客户端仅有的两种「任务进行中」消息。

**坐实：** 一个 MOCK 任务跑到一半发 `cancel`，它照样跑完了：

```
cancel was sent mid-job, but the run ended as
  {"kind":"done","result":{"ok":{"elapsed_secs":1.505489093}}}
instead of cancelled
```

**影响面：** 仅 `pnpm run dev:web`（serve 是开发期后端，不发给最终用户）。GUI 没这个问题，
因为 Tauri 的每个命令是独立调用，`flash_cancel` 不需要排在 `flash_run` 后面。

**修法：** 把 `handle_run_job` 从循环里 `tokio::spawn` 出去，循环继续读消息。连带的两件事：

1. **一条连接同时只允许一个任务**，第二个回 `error` 帧。以前这是靠「循环被阻住」隐含保证的；
   循环一旦不阻塞，`cancel.store(false)` 就会把前一个任务的取消一并清掉，必须明确拒绝。
2. **连接关闭时等任务退干净**再丢 sink，避免任务活得比连接还长、串口迟迟不释放。

前端不用改：`ws-transport.ts` 的 runJob 路径本就处理 `type === "error"`，新的拒绝会直接变成
一个 reject。（顺带观察到一个**不属于本次范围**的前端弱点：runJob 每次挂一个 message
handler，若真有人绕过 `canFlash` 等计算属性并发发两次，拒绝帧会串到第一个 handler 上。
UI 上发不出并发任务，所以是理论风险。）

**没盖到的：** `AuthorizeConfirm` 的死锁由同一处修改一并解决，但**没有直接的测试**：
Authorize 模式在 `run_job` 里早于插件查表就分发给了 `run_authorize`，绕开了注册表，
假芯片插不进去。要盖它得先给 authorize 流程开一个同类的缝隙，不在本次范围。

### serve 怎么拿到假芯片

`mock-chip` 不能写进 serve 的普通依赖（守卫测试禁止），也不能用 `#[cfg(feature)]`（serve 没声明
这个 feature）。用的是 **dev-dependency**：

```toml
[dev-dependencies]
tyutool-core = { path = "../tyutool-core", features = ["mock-chip"] }
```

cargo 在构建测试时合并 normal + dev 依赖的 feature，所以 serve 的单测里 `MOCK` 存在；而
`cargo build --release` **根本不编译 dev-dependency**，所以按构造就进不了产物。这也是守卫
测试将 `dev-dependencies` 列为唯一例外的依据。

例外是**验过的不是假设的**：`cargo build --release -p tyutool-cli` 会拉进 tyutool-serve，
若 dev-dependency 的 feature 泄进了那次构建，`src/lib.rs` 的 `compile_error!` 会当场炸。
实测：构建成功。

#### 一个必须记下的副作用：feature 统一

这个 dev-dependency 不只影响 serve 自己。CI 把三个 crate 放在**一条** `cargo test` 里跑
（`ci.yml:46`），cargo 会在这条命令内统一 feature，于是 `tyutool-cli` 的测试也看得到 `MOCK`。

实际后果：`tyutool-cli` 的 `device_list_matches_registry`（它断言 `SUPPORTED_DEVICES` 不得与
注册表漂移）当场报错，多出一个 `mock`。处理方式是在那个测试里**显式排除**它并注明原因
——假芯片本就不属于那份契约。

这里要说清楚的是：统一只发生在**测试二进制**上。`cargo build` / `cargo build --release`
不编译 dev-dependency，所以发布产物不受影响。但意味着「没人开这个 feature」不再是一个可依赖
的前提，**封锁完全靠那两道保障**。以后又有哪个 crate 断言注册表内容时，记得同样排除。

---

## 阶段三的结果：`src-tauri` 的 flash 命令终于可测

`flash_run` 是仓库里最难看懂的几十行之一（取消 Arc 的原子替换、等旧线程 3 秒、confirm
通道），而 979 行的 `lib.rs` 只有 32 个测试，这段一行未测。

用 Tauri 官方的无头 mock 运行时（`tauri::test::mock_builder`）+ `MOCK` 假设备，现在可以在
没有 WebView、没有硬件的情况下直接驱动它。四个用例：任务跑完并以 `flash-progress`
事件送达前端；中途 `flash_cancel` 生效；**启动第二个任务会把第一个取消而不是两者共用
标志位**（那段注释声称的性质，这回有证据了）；无待决确认时 `authorize_confirm_cmd` 报错。

结论：GUI 没有 serve 那个取消失效问题——但这一点以前是推断，现在是测试。

### 一处产线修改：`flash_run` 泛型化

裸写的 `AppHandle` 带 `#[default_runtime]`，实际是 `AppHandle<Wry>`，而 `Wry` 需要真实
WebView，测试里造不出来。改成 `fn flash_run<R: Runtime>(app: AppHandle<R>, ..)`，
`tauri::generate_handler!` 依旧把它解析到 app 自己的运行时。**只改签名，行为不变**，
这也是 Tauri 自己文档推荐的可测写法。

### 一个差点踩到的坑

测试最初落在了 `mod xdg_command_tests` 里——而那个模块带 `#[cfg(target_os = "linux")]`。
它在 ubuntu 的 CI 上照跑不误，但在 Windows / macOS 上会**静默消失**。已移到主 `mod tests`。
往这个文件加测试时看一眼落点所在的模块有没有 `cfg`。

### 封锁对 GUI 同样验过

src-tauri 拿假设备走的也是 dev-dependency。`cargo build --release -p tyutool_gui`（`tauri
build` 底下就是它）构建成功，假芯片若泄进去会当场编译失败。
