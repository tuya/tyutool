# 串口调试可靠性修复计划

**Spec:** `docs/specs/2026-10-19-serial-debug-reliability-design.md`

**Goal:** 修复 review 确认的八类缺陷，让已落盘的失败回归测试转绿，并覆盖桌面关闭竞态及关联的异步边界。

**Architecture:** 保留 core / Tauri / serve / Pinia 的现有职责；共享串口流语义归 core，宿主只负责连接和事件。所有 agent 共享当前工作区，按文件分工，不切换分支、不提交、不推送、不覆盖他人的改动。

**Tech Stack:** Rust、Tauri 2、Vue 3、Pinia、Vitest；不增加依赖。

## File Map

| 文件 | 负责人 | 内容 |
|---|---|---|
| `crates/tyutool-core/src/serial_debug{,_bridge}.rs` | Luna 后端 | 缺口顺序、排空契约与测试 |
| `src-tauri/src/{lib,serial_debug}.rs`、`crates/tyutool-serve/src/lib.rs`、core `lib.rs` | Luna 后端 | 关闭确认及宿主行为测试 |
| `src/stores/serial-debug.ts` 与现有测试 | Luna 会话 | 关闭、清空、过期筛选响应、保存交接 |
| `src/features/serial-debug/useSerialAutoSave.ts` 与现有测试 | Luna 会话 | 后台写盘、目录切换、清空前保存与异步顺序 |
| `src/transport/ws-transport.ts`、`src/features/serial-debug/transport.ts` 与现有测试 | Luna 会话 | WS / Tauri 关闭完成确认与监听清理 |
| `src/features/serial-debug/hex-view-renderer.ts` 与现有测试 | Luna 显示 | 原始字节一致性 |
| `src/features/serial-debug/components/SerialDebugLogView.vue` 与现有测试 | Luna 显示 | 固定导出筛选及数据范围 |
| `src/features/serial-debug/log-line-renderer.ts` 与现有测试 | Luna 显示 | 有证据支持的缓存优化 |
| `src/stores/flash.test.ts` | Luna 会话 | 修正关闭中状态的旧假设，保留烧录等待串口释放的验证 |
| 本 spec / plan | 主 agent | 计划、审查与验证记录 |

## 执行步骤

- [x] 记录现有失败测试及修复边界，明确三个文件互不重叠的执行任务。
- [x] Luna 后端：修复缺口位置与关闭排空；补充可独立运行的桌面关闭行为测试。验收：真实队列溢出不拼接缺口两侧，最后一批与未换行尾部完成归档，读取线程不被阻塞。
- [x] Luna 会话：串口会话持续自动保存；清空前保存旧队列；目录切换完成文件交接；关闭后端后再处理最后事件；屏蔽旧筛选响应。验收：离开页面仍写盘且不积压；清空不丢待写日志；目录改变实际生效；关闭及清空竞态有测试。
- [x] Luna 显示：修复十六进制人工 LF；固定导出源及上限；审查缓存的全窗口扫描成本。验收：LF / CRLF / 二进制尾部字节一致；切筛选不混数据；持续新数据不让导出无限延伸；缓存不泄漏。
- [x] 主 agent 审查：逐项对照 8 类缺陷与 10 项已落盘用例，检查新增状态的重入、失败、关闭、重新开启与清空边界；不接受删除失败用例、跳过用例或改断言迎合旧错误行为。
- [x] Luna 按审查意见修订，主 agent 再审；必要的旧测试期望调整必须解释其原先错误假设。
- [x] 主 agent 最终验证：fmt、全部 workspace 包 clippy 与测试、core 可选功能和 ts-rs / bindings、前端 lint / scripts 类型检查 / 覆盖率 / build；端口测试在允许本机端口的环境执行。
- [x] 更新本计划中的结果、测试数量与限制。没有实机时明确说明未完成实机长时间压力测试；实现验证完成不等于已经发布，文档暂不移入 completed。

## 初始测试基线

已有目标文件：前端 156 通过、8 失败；core bridge 9 通过、1 失败。新增测试共 10 项，其中共享层关闭排空契约 1 项已通过。失败属于本计划要修复的问题，不是环境故障。

## 初次方案审查要求

- WS close 原实现解除 message listener 后立即返回；必须等 `serial_debug_closed`，Tauri 增加同等有序的 `serial-debug-closed` 事件。
- 全局 drop epoch 加一次性累计计数无法区分未来多个缺口；缺口报告必须绑定到准确的成功入队消息，静默端口的末尾丢包也须及时被告知，不能拖到关闭。
- clear 使用调用点的保存水位，不能无限追逐持续输入；包括已到达前端但还没到 16ms 刷新点的块与未换行尾部。写入失败不得继续清空。
- 导出源在按钮入口固定，上限在首个响应固定，后续请求受这个上限约束；缓存优化必须保持任意历史/tab ID 顺序的正确性。

## 首轮完成结果（2026-10-19）

三个 Luna 子 agent 完成实现，主 agent 完成逐项审查和最终集成验证。原有 9 项失败回归已转绿，关闭排空契约继续通过；新增真实队列溢出、读取线程不阻塞、桌面关闭确认、重复关闭、关闭/重开互斥、清空期间原始块到达、保存失败、目录交接、串口被烧录接管、过期筛选与导出边界测试。

审查后修订了以下边界：缺口元数据随成功入队块传递；正常空闲刷盘不持有生产者锁；空会话关闭也返回确认；清空按有限保存水位等待且传播写入错误；等待期间到达的原始块与尾部继续进入保存队列；关闭确认前保持会话存活。缓存使用可见范围预算并按成员关系淘汰，覆盖非递增历史 ID。

旧测试只调整两处错误假设：十六进制测试的原始字节 fixture 显式带上原有 LF；烧录接管测试在关闭完成前保持串口会话为打开状态，并继续验证端口未释放前不会启动烧录。没有跳过测试或削弱相应的字节/端口所有权断言。

最终验证全部通过：

- 前端 65 个测试文件、971 项测试及覆盖率门槛；lint、scripts 类型检查、build。
- 全 workspace 的 fmt、各包 clippy（warnings 作为错误）与测试，包括 GUI、serve、bridge；serve 本机端口测试在允许绑定端口的环境运行。
- core 的 ts-rs 测试与 bindings 无漂移检查，以及 zip / excel / mock-chip / record-io / download 组合的 clippy 和测试。
- `git diff --check`。

尚未进行实机长时间压力测试。实现已完成本地验证，尚未提交、推送或发布；spec / plan 保留在进行中目录，发布后再归档。

## 二次审查待办（2026-10-19）

完整问题、复现证据与验收条件见 [二次审查记录](../audits/2026-10-19-serial-debug-follow-up-review.md)。上面的首轮全绿记录早于本轮新增失败回归；以下 4 项在二次审查落盘时尚未修复，现已完成，结果见文末。

- [x] SD-R2-01：导出跨 clear 混入不同归档；固定归档身份或安全取消。
- [x] SD-R2-02：Tauri close / clear 未互斥，旧尾部可写回归档；补真实宿主路径测试。
- [x] SD-R2-03：子目录切到父目录被前缀判断误判为未变更。
- [x] SD-R2-04：同秒重启自动保存复用文件名，历史重复追加。

## 二次修复执行方案（2026-10-19）

用户已授权开始修复。由三个 Luna 子 agent 实施，主 agent 审查、协调共享接口并执行最终门禁；不提交或推送。

| 任务 | 文件范围 | 方案与验收 |
|---|---|---|
| SD-R2-03、04：Luna 会话 | `useSerialAutoSave.ts` 及测试 | 精确跟踪文件所属的保存根目录；每次保存会话使用独立文件名；目录前后级、连续变更和同秒重启测试通过 |
| SD-R2-02：Luna 后端 | `src-tauri/src/serial_debug.rs` | clear 参与现有 open/close 生命周期互斥；命令和行为测试调用同一宿主路径；冲突时不修改归档，释放后能清空尾部 |
| SD-R2-01 与 clear 前端协调：Luna 显示 | `SerialDebugLogView.vue` / store 与测试、i18n | clear 开始与完成使旧导出失效；异步对话框、读页和写文件边界检查归档修订号，明确提示取消；后端 clear 被拒绝时不提前清除 UI 数据 |

审查特别要求：清空等待期间到达的原始块和尾部仍进入保存队列；异步筛选在 clear 入口失效；导出取消可以保留已写入的旧页，但绝不能追加新归档的数据；不将 core 模拟的预期错误地变成 core API 契约。

验证顺序：三项已落盘失败回归转绿 → 新增边界与宿主行为测试 → 主 agent 逐项审查并修订 → 完整 frontend 与 Rust 门禁。

二次修复审查中补充的验收：backend clear 等待期间新发起的筛选读取也必须失效；后端行号从 0 重启时，自动保存不得继续套用旧回填水位。clear 前保守失效旧回填水位，保留等待期间到达的数据。

Rust 完整门禁已于二次修复后重新通过，包含 GUI 的 43 项测试（新增两项生命周期行为测试）、其他 workspace 包、core 可选功能和 bindings 漂移检查。前端完整门禁也已通过：65 个测试文件、984 项测试及覆盖率、lint、scripts 类型检查、build。

二次修复已完成实现、主 agent 复审及全部门禁。原有三项失败回归全部转绿，新增边界与宿主行为测试通过；详情和覆盖限制见关联审查记录。尚未进行实机长时间压力测试；未提交、推送或发布，文档保留在当前目录。
