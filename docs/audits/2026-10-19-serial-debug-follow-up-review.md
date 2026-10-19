# 串口调试二次审查与修复记录

日期：2026-10-19

范围：`codex/log-latency` 工作区中尚未提交的串口调试修复及相关自动保存路径。

关联：[修复计划](../plans/2026-10-19-serial-debug-reliability.md)、[设计说明](../specs/2026-10-19-serial-debug-reliability-design.md)。

## 状态与证据边界

本记录初次落盘时只登记问题和复现证据，以下 4 项均为 P2。随后按用户授权完成修复与主 agent 审查，最终验证见文末。首轮 971 项测试通过及本记录中的 3 项失败回归，均保留为修复前的历史快照。

SD-R2-01、03、04 已通过前端临时用例复现，随后整理到现有测试文件。SD-R2-02 使用真实 core 队列、归档和 shutdown/finalize API 复现，但没有直接驱动完整 Tauri command；宿主调用顺序由源码审查确认。不能将其表述为桌面集成测试已经覆盖。

## SD-R2-01：清空期间导出可能混入两个归档的数据

- 优先级：P2；状态：已修复。
- 位置：`src/features/serial-debug/components/SerialDebugLogView.vue` 的 `streamExportChunks`，当前约 737–780 行。
- 原因：固定了筛选 ID 和首个响应的总行数，但没有固定归档身份；后续页仍读取当前归档相同 offset。清空会重建归档并重置行号，固定行数不能隔离两个归档。
- 复现：原归档有 1001 行，开始导出；第一页 1000 行已取得，第二页响应挂起；执行 `s.clear()`，模拟新归档产生至少 1001 行，再返回第二页。
- 实际：下载内容由旧归档第 1–1000 行和新归档第 1001 行组成。新归档不够长时还可能静默提前结束。
- 期望：导出只包含同一归档的数据；归档变化时可以明确取消，不能输出混合文件。
- 回归位置：`src/features/serial-debug/components/SerialDebugLogView.test.ts`。测试应允许正确取消或导出原快照，不强制必须下载文件。
- 验收场景：All 和筛选两条分页路径；保存对话框等待期间及分页写文件期间清空。

## SD-R2-02：桌面关闭与清空并发，旧尾部可能写回已清空的归档

- 优先级：P2；状态：已修复；已补命令共用宿主状态 helper 的行为测试，未直接驱动完整桌面 command。
- 位置：`src-tauri/src/serial_debug.rs` 的 `serial_debug_close`（约 264–295 行）与 `serial_debug_session_clear`（约 374–394 行）。
- 原因：close 取得生命周期 reservation 后先从状态取走 bridge，再异步停止串口和排空；clear 不参与 reservation，可能看到 `chunk_bridge == None`，只推进共享 generation 并清空归档。旧 bridge 的本地 active generation 仍可接受其旧队列，随后 shutdown/finalize 将旧尾部写回归档。
- 宿主触发顺序：close 取走 bridge → clear 看到无 bridge 并清空 → close 排空旧 bridge。
- core 复现步骤：
  1. 创建真实 archive、filters、generation 和 chunk bridge。
  2. 发送 `ready\n` 并等 sink 收到 batch，确保 worker 已启动且 active generation 为 0。
  3. 持有 archive 锁，向真实队列发送 `old-tail`；推进共享 generation，并在锁内 `archive.clear()`，再释放锁。
  4. 调用真实 `serial_debug_shutdown_and_finalize`，检查归档行数。
- 实际：期望 0 行，得到 1 行（旧尾部）。
- 期望：关闭与清空必须有明确顺序；清空成功后旧 session 数据不能重新写回。
- 测试限制：上述模拟证明 core 症状，不规定应修改 core 行为；宿主互斥/排队同样可能是正确修复。不能直接把模拟当作 core API 的错误契约测试长期加入 CI。
- 验收场景：在 `src-tauri/src/serial_debug.rs` 补实际生命周期路径的行为测试，覆盖 close→clear、clear→close，以及 clear→open；确认不会死锁，拒绝操作时前端反馈明确。

## SD-R2-03：保存目录从子目录切到父目录不生效

- 优先级：P2；状态：已修复。
- 位置：`src/features/serial-debug/useSerialAutoSave.ts` 的设置 watcher，当前约 315 行。
- 原因：`path.startsWith(newDir + "/")` 只能判断文件是否在某个目录树下，不能判断用户选择的保存根目录是否改变。
- 复现：启用自动保存，目录设为 `/logs/child`；保持串口打开，将目录改为 `/logs`，等待 watcher 和 transition 完成。
- 实际：`sessionAutoSavePath` 完全不变，后续日志继续写入 `/logs/child/<port>/...`。
- 期望：待写内容正确交接后，新路径应为 `/logs/<port>/...`。
- 回归位置：`src/features/serial-debug/useSerialAutoSave.test.ts`。
- 验收场景：子目录→父目录、父目录→子目录、相邻目录及慢写期间连续切换。

## SD-R2-04：同一秒内重启自动保存，会重复追加历史

- 优先级：P2；状态：已修复；原有问题，首轮修复未覆盖。
- 位置：`src/features/serial-debug/useSerialAutoSave.ts` 的 `startAutoSave`（约 249 行）、`src/features/serial-debug/utils.ts` 的 `makeStamp`。
- 原因：文件名只有秒级精度；同目录、同端口、同秒重启会复用相同路径，历史回填通过 `append_text_file` 追加到已存在的文件。
- 复现：固定系统时间；归档有 `archived-once` 一行；启用自动保存并等回填完成；关闭自动保存后在同一秒再次启用，等第二次回填完成。
- 实际：同一历史行向同一路径追加两次。
- 期望：重启不会向同一个文件重复回填历史；采用独立文件或明确的续写策略均应有对应验证。
- 回归位置：`src/features/serial-debug/useSerialAutoSave.test.ts`。
- 验收场景：同秒 off→on、快速关串口→重开，以及多次快速重启；不能只依赖手动等待到下一秒。

## 后续执行清单

- [x] 记录 4 项问题、触发顺序、实际行为、预期行为和证据限制。
- [x] 修复 SD-R2-01，并覆盖 All / 筛选导出与清空的归档隔离。
- [x] 修复 SD-R2-02，并补宿主生命周期行为测试。
- [x] 修复 SD-R2-03，验证目录切换和保存交接。
- [x] 修复 SD-R2-04，验证快速重启不会重复追加。
- [x] 回归转绿后重新审查，再运行对应及完整门禁；届时更新首轮验证快照。

## 修复前落盘验证（历史快照）

已将 3 项正式回归加入现有测试文件：自动保存 2 项、导出 1 项。运行这两个文件共 63 项：60 通过、3 项按预期失败，失败原因分别对应 SD-R2-01、03、04。没有 `.skip` / `.todo`，没有修改产品逻辑。SD-R2-02 的复现步骤与边界已在上文记录，直接宿主路径测试仍列为待办。两个修改的测试文件通过 Prettier 检查，`git diff --check` 通过。

复跑命令：

```sh
pnpm exec vitest run src/features/serial-debug/useSerialAutoSave.test.ts src/features/serial-debug/components/SerialDebugLogView.test.ts
```

## 修复结果与最终验证（2026-10-19）

三个 Luna 子 agent 分别完成自动保存、桌面生命周期、导出与前端清空协调，主 agent 完成审查、修订要求和完整门禁验证。

- SD-R2-01：store 的归档修订号在 clear 进入和退出时变化；导出在异步对话框、每次读页及写入边界校验修订号，失效后以中英文提示取消。All / 筛选分页、对话框等待、清空期间新建导出和慢写场景均有行为测试。桌面上已写入的旧页可以保留，但不继续写入新归档内容；本轮不提供原子文件替换或后端快照协议。
- SD-R2-02：clear 使用 open/close 共用的生命周期 reservation，冲突时不修改 archive/generation。新增测试使用命令实际调用的 `clear_session_state`，结合真实 bridge 入队尾部、close 排空和 archive gate 验证互斥及清空结果；没有构造 `AppHandle` 或实际串口驱动完整 command。前端等待后端 clear 成功后才重置日志，拒绝时保留原日志并提示错误。
- SD-R2-03：按当前文件所属保存根目录精确比较，不再根据文件路径前缀猜测。父子目录、相邻目录和旧文件慢写期间连续切换均通过测试。
- SD-R2-04：每次保存文件名增加 UUID，保留可读时间戳，避免同秒/同毫秒重启及不同组件实例复用文件名。固定时间的自动保存重启及串口关闭重开测试通过。

主 agent 复审补充了两个保护：clear 等待期间发起的筛选读取在成功后再次失效；后端可能重建归档前先保守失效旧自动保存回填水位，防止新行号被误判为已保存。两者均有 gate 回归测试。

最终全部通过：

- 前端 65 个测试文件、984 项测试及覆盖率门槛；lint、scripts 类型检查、build。
- Rust 全 workspace 的 fmt、clippy 和测试，包括 GUI 43 项测试；core 的 ts-rs / bindings 无漂移及 zip / excel / mock-chip / record-io / download 组合检查。需要本机端口的 serve 测试在允许绑定端口的环境运行。
- `git diff --check`；修改的前端文件通过 Prettier 检查。

尚未进行实机长时间压力测试。本次修复未提交、未推送、未发布。
