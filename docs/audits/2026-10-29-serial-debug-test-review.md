# 串口调试测试研究记录

本文按完整用户链路记录串口调试测试保护、已确认缺陷与验证边界。最终验收快照已由主审确认。

## Rust 收发、归档与桥接

- `crates/tyutool-core/src/serial_debug.rs` 的 `SerialDebugArchive` 将 RX/TX 分方向累积为行、落盘并以行索引分页读取；新归档的每条 TX/RX 行通过可选 `rawBytes` 保留原始输入字节（包括 CRLF、NUL、非法 UTF-8，且不添加合成 LF），Sys 行不带该字段。JSON 数组序列化开销计入归档 cap；`.idx` 仍按实际 NDJSON 编码长度定位。旧 text-only JSON 缺少该可选字段时仍可读取，但其字节无法恢复。
- 回归覆盖 UTF-8 跨 chunk 解码、非法 GBK 原字节与 lossy 文本并存、RX/TX 字节分页及 filter match page、gap/close 尾部无合成字节、超过 4096 字节强制分行后与 close tail 拼接还原输入、旧 text-only serde 兼容，以及 Sys 的 `rawBytes: None`。
- 同文件还新增 archive cap 为 0 时仍可写、limit 为 0 返回空页、`u64::MAX` start/limit 饱和分页和并行 archive 会话隔离测试。既有分页等价测试覆盖不同 offset、limit、批次边界及 cap/drop sentinel 场景。
- `crates/tyutool-core/src/serial_debug_bridge.rs` 的桥接测试验证批内 chunk 顺序/方向及 `archived_before` 位置、reset generation barrier 丢弃 reset 前 chunk、consumer shutdown 后迟到发送安全忽略，以及独立 bridge/archive 不共享行号与内容。
- 新增 Rust 用例使用真实临时归档目录中的日志与索引文件，并通过真实 bridge worker 线程/队列验证顺序、reset 和 shutdown 行为；它们不是 fake archive。core 现有测试还包含 PTY 场景。新增用例名可在上述文件定位：`utf8_codepoint_split_across_chunks_is_decoded_after_line_completion`、`crlf_is_removed_and_cr_only_is_preserved_inside_text`、`non_utf8_gbk_bytes_are_lossy_in_text_but_archive_does_not_claim_decoding`、`clear_discards_unterminated_pending_bytes`、`read_page_clamps_start_and_saturating_limit_at_archive_end`、`bridge_preserves_chunk_order_and_direction_within_a_batch`、`reset_discards_pre_reset_chunks_before_accepting_new_generation`、`send_after_consumer_shutdown_is_safely_ignored`、`separate_bridge_archives_do_not_share_lines_or_positions`。本任务没有新增 PTY 或真实设备测试。

## Tauri 与 WS

- `src/features/serial-debug/transport.ts` 将 Tauri command/event 和 WS transport 适配到统一接口。`src/features/serial-debug/transport.test.ts` 新增 Tauri command 名称及参数映射、disconnect/filter/archive-capped/chunks-dropped 事件载荷、订阅独立退订、append sys line 归档位置（null/line number）测试，并验证 WS transport 的事件回调路由。
- 已确认 WS 分页挂起缺陷：`src/transport/ws-transport.ts` 中 `serialDebugFilterReadMatches()` 与 `serialDebugSessionReadPage()` 原先只监听 message；WebSocket 在分页响应前 close/error 时 Promise 不结束，调用方会一直 pending。
- 当前实现为两个分页请求都注册 close/error handler，并在成功响应、服务端 error、socket close/error、同步 `send` throw 时统一移除 message/close/error listeners 后 resolve/reject。`src/transport/ws-transport.test.ts` 的 `rejects and cleans up a %s page request after socket %s` 参数化覆盖 filter/session 两类请求分别遇到 close/error/send throw；另有 request_id 并发关联、request-scoped server error 和 pending session page close 即时 reject 测试。

## Store

- `src/stores/serial-debug.ts` 是 UI 会话状态入口，负责 RX/TX 行组装、pending 尾行、archive position、历史视图、过滤器与 auto-save 队列的协调。
- `src/stores/serial-debug.test.ts` 已覆盖发送编码/历史、clear 对 live 与 filter 状态的重置、过滤分页与旧 session 结果隔离、history 窗口进入/向前向后翻页/预算/连续性，以及 close 时最终 chunk 的收尾。`src/stores/serial-debug-workspace.test.ts` 覆盖 workspace 字段校验、默认值、枚举与数值范围规整。
- 本轮 store history 断言未平行重复，新增直接 composable 测试以隔离分页 generation 与 loading 生命周期。

### 实时未完成行预览

- Store 将未遇到 LF 的 TX/RX 尾部作为独立预览投影，保留已完成 `lines` 不变。下一次 16 ms 批处理即可显示 prompt/二进制响应；后续字节更新预览并生成新 display id，使用输入 chunk 的时间戳。`appendChunk` 直达路径使用相同预览刷新逻辑；空字节事件不刷新预览。TX 与 RX 各自累积，UTF-8 跨 chunk 在完整字节到达后纠正文本，预览的 `rawBytes` 始终是当前原始字节。
- 换行只生成一条已完成行，预览不进入归档、filter 或 auto-save 队列。All 视图的分页导出仍以已完成归档为界，故不包含 partial。drop/close 会收尾已有尾部，clear 成功清空；clear 失败保留原显示内容。All 投影受 `logWindowLines` 总行数预算约束。
- Store 测试覆盖 split UTF-8、TX/RX 独立、renderer cache 新 id、预览不重复进入完成行/auto-save、window budget、drop/close 与 clear 失败边界。组件级 `SerialDebugLogView.test.ts` 覆盖 `appendChunk` 后 ASCII 提示符可见、切换 Hex 后显示预览原始字节、补字节并遇 LF 后只完成一次。
- 这些是 fake transport/store 与 happy-dom 组件自动化，不代表物理串口或长时间实机验收；本次未进行实机长测。

### 自动保存积压边界

- Store 对 accepted pending queue 强制 8 Mi `estimatedChars` 估算字符预算（不是 8 MiB 实际 RAM）与 32768 行上限。任一上限溢出会关闭 `autoSave` 并设置 gate，之后的行不再入队；一个 `serialDebug.autoSave.queueOverflow` 系统提示告知自动保存已停止、文件可能不完整、实时日志仍继续。已接受队列保留，并在 writer 可用时排空到原 session path。
- Composable 在队列达到 128 Ki estimatedChars 或 256 行时合并调度一个 macrotask flush。每个非 drain-all turn 最多写 16 批后让出事件循环，再对固定 sequence 继续，不等待下一个 5 秒间隔。
- `useSerialAutoSave.test.ts` 的 `coalesces a high-rate short-line burst into batched writes without a 5 s wait` 验证 50,000 条短行恰好写入一次；`bounds a stalled writer, warns once, and drains accepted lines to the old file` 验证慢写时队列有界、单次提示以及 accepted 队列全部写入旧路径。clear 前 ready timer 取消回归也已通过。50,000 行输入与慢写 gate 均为软件模拟，没有进行实机压力测试。
- 上述均为确定性软件自动化验证，不代表实机或长时硬件压力测试。
## 历史、过滤、导出与自动保存

- `src/features/serial-debug/useSerialDebugHistory.ts` 以 generation 隔离归档窗口请求，按 `HISTORY_PAGE_SIZE` 分页，检查连续性并受 history budget 限制。新增 `src/features/serial-debug/useSerialDebugHistory.test.ts` 直接验证 stale probe/range、会话切换时旧请求 finally/rejection、active failure loading 收尾、空 archive、稳定 display id、entry offset、旧页连续性/重锚、session start/end 与 jump-to-start 取消。
- 已确认并修复 history stale response 缺陷：tail range 完成后曾先 `applyWindow()` 再由调用方检查 generation，导致 clear/exit 后旧会话数据复活。现在 probe 和 range 写状态前均检查 generation；过期请求的 catch/finally 不会上报旧错误或清除新请求 loading，exit 会同步复位 loading。
- 过滤器数据由 Rust archive/filter index 分页，经 Tauri/WS 传输到 store；`archiveLineToLogLine` 已把可选归档 `rawBytes` 转成 `Uint8Array`，history/filter 共用该映射。renderer 回归通过此 mapper 验证新归档原始字节和旧 text-only 行的 LF fallback。对应保护见 `src/stores/serial-debug.test.ts`、`src/transport/ws-transport.test.ts` 与 `src/features/serial-debug/transport.test.ts`。本轮没有修改过滤器实现。
- 全量导出分页及 archive 增长期间使用首个 page snapshot 的保护见 `src/features/serial-debug/components/SerialDebugLogView.test.ts`；archive sentinel 翻译与导出格式见 `src/features/serial-debug/archive-line-text.test.ts` 及同目录组件测试。
- auto-save 的 live flush、启用时 archive backfill、切换/关闭会话、clear 边界、失败恢复、队列预算与 overload 交接见 `src/features/serial-debug/useSerialAutoSave.test.ts` 和 `src/stores/serial-debug.test.ts`。

## 验证证据与边界

- 前端最终快照：68 个测试文件、1069 项通过（本轮三项修复前 1054，增加 15）；coverage 为 statements 83.34%、branches 76.21%、functions 88.04%、lines 84.32%。lint、scripts typecheck、build 与 Tauri version parity 均通过。
- 本文作者另行验证：`pnpm run build` 通过；`pnpm exec vitest run src/features/serial-debug/transport.test.ts src/features/serial-debug/useSerialDebugHistory.test.ts` 通过，2 文件、38 用例。构建输出含已有 dynamic import/chunk size 警告，不影响成功退出。上述主审快照为最终全量结果。
- Rust 最终快照：普通 core 414 个单测 + 2 个集成测试、CLI 60 个测试；optional feature 组合 446 个单测 + 2 个集成测试；serial 专项 67 个测试。GUI、serve、Bridge 测试，`cargo fmt --all --check`、全部 clippy、`ts-rs`/bindings drift gate 均通过。
- 三项修复合计新增 17 项：前端 15、Rust 2。该计数不把多项既有测试增强算作新增。
- 前端 fake WebSocket/transport、确定性 gate、50,000 条短行与慢写受控 gate、happy-dom 组件测试均为软件模拟，不等价于物理串口或电气验收。Rust archive 用例读写临时归档文件/索引并运行 bridge worker 队列；core 既有 PTY 测试属于软件层串口交互证据。本轮未进行目标设备实机或长时压力测试。目标板 RX/TX、电气信号、拔线行为和持续运行仍需设备验收。
