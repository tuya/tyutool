# tyutool 多维度代码审计报告

| 项目 | 内容 |
|---|---|
| 审计日期 | 2026-08-04 |
| 当前分支 | `refactor/v3`（版本 3.2.8，各源一致） |
| 审计方式 | 只读审计，6 个并行子代理按维度分头调研；**未修改任何源码** |
| 维度覆盖 | ① Rust 后端 ② 前端(Vue/Pinia/transport) ③ 架构与约定 ④ 测试 ⑤ 文档一致性 ⑥ 构建/CI/依赖/安全 |
| 证据标准 | 每条发现均带已核对的 `文件:行号`，避免臆测 |

---

## 一、执行摘要

整体结论：**项目工程质量相当扎实**——无 `unsafe`/`todo!`，阻塞式串口 I/O 全部正确地放到专用线程 / `spawn_blocking`，通道有界，注册表与前端 manifest 11/11 对齐，版本号 6 处源全部一致，无密钥泄漏、无构建产物入库，Tauri capability 收敛得当。

**没有 Critical 级问题。** 主要可改进点集中在三类：

1. **安全契约的自相矛盾**（最高优先级）：日志/持久化/错误信息里出现敏感凭据，与项目自身的 Logging Contract 冲突。
2. **文档与代码漂移**：README 的芯片/设备列表严重滞后，`--verbose` 帮助文本与实际日志级别不符。
3. **健壮性 & 测试缺口**：workspace 加载缺少校验、LN882H 闪烁逻辑零测试、若干 `.lock().unwrap()`。

### 严重度汇总（去重后跨维度）

| 严重度 | 数量 | 代表项 |
|---|---|---|
| Critical | 0 | — |
| High | 4 | authkey 泄漏进用户可见错误/日志；`--verbose` 与 `.level(Info)` 矛盾；README `-d` 列表（3 vs 11）；LN882H 闪烁逻辑零测试 |
| Medium | ~19 | CSP 关闭；重复 crate 大版本；CI 仅 Linux；auth 凭据明文持久化；workspace 加载无校验（×3）；batch 绕过 port-manager；硬编码中文绕过 i18n；expect panic；等 |
| Low | ~18 | 自更新可预测临时路径；无界 WS sink；`matchMedia` 监听未清理；死代码 `ThemeStyle`；AGENTS.md `.rs` 措辞错误；等 |
| Info | 多 | 类型镜像无漂移；i18n key 100% 对齐；CLI docs↔code 对齐 PASS；日志路由/裁剪符合契约；无密钥；等 |

---

## 二、最关键发现（建议优先处理）

### 🔴 H1. 批量授权校验失败会把密钥 `authkey` 写进用户可见错误/日志  ✅ 已修复
- `crates/tyutool-core/src/authorize.rs:1673`（以及旧固件分支 `:1941`）
- 写入/回读不匹配时，错误消息形如 `format!("Verify failed: wrote ({uuid},{authkey}), read ({rb_u},{rb_k})")`，被塞进 `BatchAuthRowUpdate::StepFailed{error}`（GUI 批量表里直接展示）和 `FlashError::Plugin(msg)`（返回给用户）；T5AI 分支还通过 `log::warn!("... reason={msg}")`（`:1674`）写进日志文件，而 GUI 日志可被 `export_logs_zip`（`src-tauri/src/lib.rs:2906`）导出。
- **直接违反 AGENTS.md Logging Contract**（"Displaying `AuthReadComplete` credentials as plain log text in GUI"）。
- 讽刺的是，单设备流程在 `:1234` 处理正确（"AuthKey mismatch" / "UUID mismatch (wrote {uuid}, …)"），批量路径是不一致的 bug。→ 改为只输出长度/掩码信息。

### 🔴 H2. `--verbose` 帮助文本与日志过滤器矛盾，debug 日志永不落盘
- `crates/tyutool-cli/src/main.rs:24`（帮助："Also write developer logs to stderr (always writes to log file)"）vs `main.rs:416` 的 `.level(log::LevelFilter::Info)`。
- fern 在该级别过滤整条链，`log::debug!`/`log::trace!`（协议帧、重试计数等"开发者诊断"）被丢弃、**永远进不了日志文件**——既违背帮助文本，也违背 Logging Contract（"log::* macros → developer diagnostics (file)"）。
- 处置：把 dispatch/文件级别提到 `Trace`+，或修正文档/帮助。

### 🔴 H3. README 的设备/芯片列表严重滞后（3 vs 11）  ✅ 已修复
- `README.md:101` / `README_ZH.md:101`："Supported `-d` values: `bk7231n`, `t2`, `t5ai`"，而 `main.rs:173-176` `SUPPORTED_DEVICES` 与 `docs/cli.md` 列了 **11** 个设备；`README.md:15-17` 芯片表还缺 **ESP32-P4** 与 **LN882H**。
- 与 AGENTS.md 指定的权威 CLI 参考 `docs/cli.md` 直接矛盾。两份 README 同步漂移。
- 注：`docs/cli.md`（CLI 命令同提交更新契约的"硬门"）当前是 **PASS** 的——11 个子命令/flag/默认值全部对齐。

### 🔴 H4. LN882H 闪烁逻辑完全无测试  ✅ 已修复
- `crates/tyutool-core/src/plugins/ln882h/mod.rs`（404 行，零 `#[cfg(test)]`）。内含可纯函数化的逻辑：`parse_hex_addr()`、`resolve_segments()`、4 KiB 对齐校验（`start % SECTOR != 0 || length % SECTOR != 0`）；同目录 `protocol.rs` 只覆盖了 `crc16`，XMODEM 状态机未测。→ 对齐/分段 bug 会静默上线，是所有插件里测试最薄弱的。

---

## 三、做得好的地方（亮点）

- **类型化错误传播**：`FlashError` 贯穿；无 `unsafe`/`todo!`/`unimplemented!`；`unreachable!` 仅守护 `FlashMode::Authorize`（且有 `authorize_mode_dispatches_before_chip_lookup` 测试兜底）。
- **并发纪律**：阻塞 I/O 走专用线程 / `spawn_blocking`；mutex 不跨 `.await`；chunk 桥用有界 `sync_channel`，消费端不回调 `send_chunk`，不会自死锁；`flash_cancel` 会唤醒被阻塞的确认提示。
- **注册表 ↔ 前端 manifest**：11/11 对齐，大小写一致，`t5`→`T5AI` 别名双侧一致，`FlashMode::Authorize` 在芯片查找前短路。
- **类型镜像**：`flash-ipc-types.ts` / `serial-debug/types.ts` / `batch-flash-auth/types.ts` 与 Rust 结构逐字段对齐，且带 Rust 源注释；未发现漂移。
- **i18n key 对齐**：`en.json` 与 `zh-CN.json` 均 694 个扁平 key，**0 个单侧缺失**。
- **安全姿态**：无硬编码密钥；Tauri capability 收敛（无 `fs:`/`shell:` 滥开，自定义日志命令仅注册 `invoke_handler`）；归档解压遍历安全（先读进 `Vec<u8>` 再校验 SHA-256）；日志查看路径校验严格。
- **版本一致**：6 处源全部 `3.2.8`，`scripts/bump-version.mjs` 单点同步。
- **构建产物不入库**：`target/`、`dist/`、`.tmp/`、`node_modules/`、`gen/schemas` 均被忽略且未被跟踪；`Cargo.lock`/`pnpm-lock.yaml` 已提交（二进制 workspace 正确）。
- **归档解压 / 路径校验 / 重试有界**：均正确。

---

## 四、按维度详细发现

### A. Rust 后端（tyutool-core / tyutool-cli / src-tauri）

**错误处理**
- **[High]** authkey 泄漏 → 见 H1。
- **[Medium]** 启动期 `.expect()` 导致 panic：`src-tauri/src/lib.rs:3226`、`:3230`（`SerialDebugArchive::create(...).expect(...)`），CLI 开发服务器 `crates/tyutool-cli/src/serve.rs:283`、`:286` 同样。app-data 目录权限/磁盘满会让 GUI 以裸 panic 中断启动，而非用户对话框。建议返回错误或回退临时目录。
- **[Low]** CLI 自更新模块大量用 `Box<dyn std::error::Error>`（`update.rs` 多函数、`monitor.rs:42`），偏离项目的类型化 `FlashError` 风格，HTTP/SHA/解压等失败模式对调用方不可区分。
- **[Low]** 更新命令 mutex 中毒处理不一致：`src-tauri/src/lib.rs:1850,1861,1915,1936,1945` 用 `.lock().unwrap()`，而同文件其它命令（`flash_run`/batch/serial-debug）一律 `.lock().map_err(...)?`。
- **[Info]** `FlashError` 把插件失败收口为 `Plugin(String)`，丢失结构化信息——但便于展示且跨插件一致，无需处理。

**日志契约**
- **[High]** 见 H1。其余**全部 clean**：banner 经共享 `diagnostics::log_session_banner`（CLI `main.rs:473` + GUI `lib.rs:3254`）；`FlashPhase::Other(String)` 仅用于回退渲染，新阶段均用类型化变体；凭据经 `AuthReadComplete`/`AuthConflict` 事件路由，CLI 仅打印到用户自己的终端。

**并发 / unsafe**
- **[Medium]** dev WS 服务器 `.lock().unwrap()` 级联失败：`crates/tyutool-cli/src/serve.rs`（~22 处，如 `364,396,408,...,684`）。任一 panic 毒化 mutex 后，后续所有 serial-debug 请求都会 panic；影响限于 `dev:web`，但同模块的 Tauri 端刻意用 `.map_err(...)?`，建议对齐。`serve.rs:545` 还有连续两次 unwrap。
- **[Low]** 生产 batch-auth 状态 `.lock().unwrap()`：`src-tauri/src/batch_auth.rs:324,357,373,388,415`，同样的级联风险（在已发布的 GUI 内）。

**插件 / 注册表 / 资源 / 可移植性**
- 全部 clean。注册表↔前端一致；无死插件/重复协议栈；串口在所有路径释放；重试有界（固定退避，可接受）；`#[cfg(target_os)]` 分支带 `not(any(...))` 兜底，无硬编码路径泄漏进 core。
- **[Low]** 自更新二进制用可预测临时路径 `crates/tyutool-cli/src/update.rs:158-162`（`temp_dir().join("tyutool_cli_new[.exe]")`）：字节虽在写盘前做 SHA-256 校验，但随后写到固定可写名再 `self_replace`，存在 TOCTOU 窗口。改用随机名（`tempfile`）。
- **[Low]** 最终 WS sink 通道无界：`serve.rs:737`（`UnboundedSender<ServerMessage>`）及 `:946` run_job 进度通道；慢客户端可能让事件堆积（实际风险低）。
- **[Info]** `crates/tyutool-core/src/tuya_dev_usb.rs:33-43` 的 `USB_PORT_ROLE_RULES` 为空 `&[]`（按文档刻意保留验证前为空），导致 `infer_usb_port_role` 目前恒返回 `None`。非死代码，但功能未启用。

---

### B. 前端（src/**，Vue 3 / Pinia / transport）

**Tauri 门控**
- **[Medium]** `pickAutoSaveDir()` 未做 `isTauriRuntime()` 门控即调 Tauri dialog：`src/stores/serial-debug.ts:1033-1042`。web/dev 模式下会抛错或卡住；同类调用点（`BatchAuthConfig.vue:26` 等）都先门控，此处漏了。
- **[Info]** 全仓无顶层静态 `@tauri-apps/*` import，均为动态 `await import(...)`，✅。

**状态 / 响应式 / 清理**
- **[Medium]** `listPorts()` 拆掉共享 WebSocket，孤立 serial-debug 会话（web/dev）：`src/transport/ws-transport.ts:202-203` 在重连前 `closeCurrentConnection()`。flash 与 serial-debug 共用同一 `wsTransport` 套接字，`app-init.ts` 启动即 `void flash.refreshDevice()`（以及手动刷新）会关闭 serial-debug `chunkHandler`（`:411`）所附的套接字；旧监听随旧套接字消亡且不会重绑到新套接字，web 模式下 RX/TX 静默停流。
- **[Low]** `runJob` 帧解析无 try/catch：`ws-transport.ts:288` `JSON.parse(ev.data)` 无保护，同文件其它 handler 全部包了。
- **[Low]** `matchMedia('prefers-color-scheme: dark')` 监听从不移除：`src/stores/settings.ts:265-272`（应用级单例可接受，但无 teardown）。
- **[Info]** batch 离开忙态时按设计保留监听：`BatchFlashAuthPage.vue:60-61`；"走开且不再回来"才泄漏（重挂载安全，影响有限）。

**端口管理归属**
- **[Medium]** batch-flash-auth 直接开串口、绕过 `usePortManagerStore`：`src/stores/batch-flash-auth.ts` 内 grep `usePortManagerStore|acquire|release` 为空，`startAuth()`（`:414`）→ `batch_auth_start` 在 Rust 侧开 N 个口，前端无 claim/release、无 `onReleaseRequest`。与 serial-debug/单点 flash 占用同端口时无应用内冲突仲裁（且 `PortClaim` 当前是单端口模型，属架构性不匹配，需明确决策而非静默）。

**workspace 持久化**
- **[Medium]** 敏感 TuyaOpen 凭据明文持久化：`src/stores/flash-workspace.ts:43-44`（`authorizeUuid`/`authorizeAuthKey`），由 `saveFlashWorkspaceToStorage`（`:222`）写入 `settings.json`/`localStorage`，`flash.ts:916-917` 还原。与契约要求的"凭据走安全 modal"自相矛盾——同一批密钥未加密落盘、跨会话留存。
- **[Medium]** serial-debug workspace 加载只校验版本字节：`serial-debug-workspace.ts:52-72`，`sendHistory`/`hexBytesPerRow`/`dataBits`/`parity`/`stopBits` 全靠 cast 信任。对照 `flash-workspace.ts:104-199` 逐字段校验+钳制。损坏/schema 漂移数据会崩 UI。
- **[Medium]** batch-flash-auth workspace 加载零校验：`batch-flash-auth-workspace.ts:48-95`，`cumulative`/`filter.blockedPorts`/`authConfig`/`sharedConfig` 直 `(await store.get<T>) ?? null` 进响应式状态。
- **[Low]** 存储文件位置不一致：`serial-debug-workspace.ts:35,67` 用 `.tyutool-workspace.dat`，其余用 `settings.json`，无文档理由，增加导出/备份复杂度。

**类型镜像 / 死代码 / i18n**
- **[Info]** 镜像类型无漂移；`authorizeStorage` 声明在 TS payload（`flash-ipc-types.ts:30`）却从不经 WS 发送（`ws-transport.ts:266-282` `wireJob` 遗漏），web 模式 authorize 恒默认 `kv`（与 `flash.ts:513` TODO 一致，但值得加注释）。
- **[Low]** 注释掉的 `ThemeStyle` 子系统是死代码，散落三处：`settings-utils.ts:14,56-57`、`settings.ts:13-15,27,32-34,197-225`、`settings-utils.test.ts:246-271`。
- **[Medium]** 硬编码中文绕过 i18n（无论所选语言都显示中文）：
  - `src/features/firmware-flash/useFlashConnection.ts:149`（WS 连接失败日志）；
  - `src/transport/ws-transport.ts:110`（`deviceReset` 超时）、`:159`（`serialDebugDeviceReset` 超时）。
- **[Info]** 固件 flash 输入校验充分（`validate-operation.ts`：UUID=20、AuthKey=32、"二者要么都有要么都无"、hex 范围）。

---

### C. 架构与约定

**层级边界** — ✅ Clean：前端无任何 flash 逻辑泄漏（仅类型镜像/显示串/UI 参数）；`scripts/` 仅编排；`vite/` 放开发插件、`src/config/` 放共享开发常量，跨边界 import 为 0。

**芯片 manifest ↔ 注册表** — ✅ Clean：11/11，大小写一致，`AUTH_ONLY` 处理符合文档。

**命名约定** — ⚠️
- **[Medium]** AGENTS.md `.rs` 规则自相矛盾：`AGENTS.md:254` 说 "`.rs` 文件 kebab-case"，例子 `serial_debug.rs` 却是 snake_case，且**全仓所有 `.rs` 都是 snake_case**（Rust 惯例正确，是**规则措辞**错了，应改为 snake_case）。
- **[Medium]** 10 个 camelCase `.ts`（`use*` composable 与 `confirmDialog`/`toastState`/`useAutoUpdate` 等）与".ts kebab-case"规则不符——要么加 composable 豁免条款，要么统一改名。
- **[Low]** 8 个与 `.vue` 同名的 `.test.ts`（PascalCase）技术上违反".ts kebab-case"但符合"测试=源名+.test.ts"，两条规则在 Vue 组件上冲突。
- **[Info]** `_cmd` 后缀、kebab-case 事件名、特性目录、`invoke` 名↔`invoke_handler` 注册——全部一致。

**Tauri IPC 镜像 / 分支模型 / 顶层一致性**
- **[Info]** 镜像类型注释齐全；`CLAUDE.md` 是单行 `@AGENTS.md` include，零分歧。
- **[Medium]** README 芯片表/`-d` 列表滞后 → 见 H3。
- **[Low]** 长期分支 `origin/open_cli`、`origin/release/v2` 不符 `<initials>/*`（疑似遗留/发布分支）。

---

### D. 测试

**覆盖缺口**
- **[High]** LN882H 闪烁逻辑零测试 → 见 H4。
- **[Medium]** `batch-flash-auth-workspace.ts:42-65` 迁移逻辑未测（LEGACY→新 key 一次性迁移 + re-save）；与已测的 `flash-workspace.test.ts` 不对称。
- **[Medium]** `serial-debug-workspace.ts`（73 行 load/save）无测试。
- **[Medium]** `phase-styles.ts:~39` 的 `phaseKey()` 纯函数未测（string/object/null/空对象分支），是进度条样式查表的入口。
- **[Low]** `useAddrRangeError.ts`、`useFlashLog.ts`（500 行环形缓冲 splice 未验证 off-by-one）未测。
- **[Info]** 关键路径已覆盖：`flash_table.rs`(4)、`frame.rs`(9)+`command.rs`(10)、`authorize.rs`(40)、`registry.rs`、`serial.rs`(14)、`beken/ops.rs`(42)、`esp/common.rs`(13)；auth 测试用 `MockAuthIo` 注入内存 IO 双，不依赖真机。

**测试质量** — 良好：纯逻辑测试断言具体值、覆盖负路径；无明显空断言/纯 happy路径测试；mock 较多但与纯逻辑单测互补。

**命名/位置**
- **[Medium]** 孤儿测试 helper：`src/stores/__test__/setup.ts` 导出 `createTestFlashStore()`，全仓**零引用**，且 `__test__/` 违反"测试就近"约定、也不会被 vitest 收集（glob 为 `src/**/*.test.ts`）。删除或就近迁移。
- **[Medium]** 同源的并行测试文件：`src/stores/settings.init.test.ts` 测的是 `settings.ts`（已被 `settings.test.ts` 覆盖），违反"已有就追加，别开并行文件"。

**配置**
- **[Medium]** `vitest.config.ts` 无 `coverage.thresholds`，覆盖可静默回退无 CI 拦截。
- **[Low]** 15 个测试用 `// @vitest-environment happy-dom` 覆盖全局 `node`，与 AGENTS.md"no DOM"措辞不符（务实但需在文档与实现间二选一）。

---

### E. 文档一致性

- **[Info]** **CLI 命令文档↔代码：PASS**（11 子命令/flag/默认值全对齐）——AGENTS.md 的"硬门"满足。
- **[Info]** issue 模板日志路径、日志路由表（CLI=stderr+fern 文件；GUI=tauri-plugin-log 10MB+KeepAll+Debug；web=CLI 文件+WS JSON）与代码一致。
- **[High]** `--verbose` 矛盾 → 见 H2。
- **[High]** README `-d` 列表滞后 → 见 H3。
- **[Medium]** README "Supported Chips" 表缺 LN882H/ESP32-P4（`README.md:15-17`）。
- **[Medium]** README 推荐无效环境变量：`README.md:159-161` `RUST_LOG=debug tyutool write …`，但 CLI **无** `RUST_LOG`/`env_logger` 处理，fern 级别硬编码 Info；正确杠杆是全局 `--verbose`。该指令静默无效。
- **[Low]** `docs/cli.md` 示例版本落后一代（`3.2.7` vs 实际 `3.2.8`，`:19`、`:307`）。
- **[Low]** `reset -d` 未被 `chip_value_parser()` 校验（`main.rs:114-116`），`reset -d <任意值>` 被接受。
- **[Low]** issue 模板仅给 Linux 路径示例，macOS/Windows 用户可能认不出。
- **[Low]** `flash.authOfficialIntro` 在 en/zh **均为空串**（`en.json:260`/`zh-CN.json:260`），确认是否遗漏翻译。
- **[Info]** CHANGELOG 连贯且最新（`[3.2.8] 2026-07-28`）；i18n key 100% 对齐。

---

### F. 构建 / CI / 依赖 / 安全

**依赖（前端）**
- **[Low]** `jsdom` 声明却未用：`package.json:61`（所有 DOM 测试用 `happy-dom` 指令），~30 个传递依赖白带；`src/stores/__test__/setup.ts:2` 还有过时注释。
- **[Low]** Tauri 插件版本规格粒度不一（`^2` vs `^2.8.0`），插件同步发版，`^2` 可能静默跳小版本。
- **[Info]** `eslint` v10（较新大版本，`^10` 下小版本可能改变 lint 结果）；lockfile 一致。

**依赖（Rust）**
- **[Medium]** `Cargo.lock` 多 crate 双大版本并存（增编译/体积）：`thiserror`(1.0.69 vs 2.0.18)、`reqwest`(0.12.28 vs 0.13.2)、`dirs`(5.0.1 vs 6.0.0)。前三个可直接修：core 提 `thiserror=2`、cli 提 `dirs=6`。
- **[Low]** `espflash`(core) 传递依赖最重；`tokio = ["full"]`(cli) 对阻塞式工具偏宽。
- **[Info]** 根 `Cargo.toml` 无 `[workspace.package]`/`[workspace.dependencies]`，版本/共享 crate 靠 `bump-version.mjs` 跨四个文件手动复制（目前一致，但 workspace 继承可让漂移从结构上不可能）。

**版本一致性** — ✅ 6 处全部 `3.2.8`，无漂移。

**Tauri 权限 / 安全**
- **[Medium]** CSP 关闭：`src-tauri/tauri.conf.json:30` `"csp": null`。虽加载本地 bundled 资源，但 null CSP 意味着任何注入/转义的可操作标记（固件派生串、日志、i18n）可加载任意资源/访问任意源。建议至少 `default-src 'self'; ...`。
- **[Info]** capability 收敛合规（无 `fs:`/`shell:` 滥开，自定义日志命令无冗余条目）；updater pubkey 公开安全；`devUrl` 锁 `localhost:1420`。

**CI**
- **[Medium]** CI 仅 `ubuntu-latest`（`.github/workflows/ci.yml:30,:44`），对一个跨平台桌面应用（Windows 为主用户群）不充分；Windows 路径处理/MSVC 构建未在 CI 验证；`release.yml` 有跨平台构建矩阵但也不跑 `cargo test -p tyutool_gui`。`clippy` 仅 core+cli（不含 `tyutool_gui`）。
- **[Low]** Actions 用可变 ref 而非 SHA（`checkout@v6`、`Swatinem/rust-cache@v2`、`dtolnay/rust-toolchain@stable`、`softprops/action-gh-release@v3` 等）；标准做法但属供应链加固缺口，建议发布关键工作流把第三方 action 钉 SHA。
- **[Info]** release 密钥处理正确（全走 `secrets.*`，macOS keychain 用 `uuidgen` 密码，tag-gated，`permissions` 按作业收敛）。

**密钥 / 提交物**
- **[Info]** 无硬编码密钥/token/私钥（grep 全为 `secrets.*` 引用或标识符名或测试夹具或 i18n 标签）。
- **[Low]** `sync-to-gitee.yml:22` 硬编码用户名 `flyingcys`（非密钥，可维护性气味）且无分支过滤地全量 force-mirror 所有分支/标签。
- **[Info]** 构建产物全部忽略且未跟踪；`Cargo.lock`/`pnpm-lock.yaml` 已提交（正确）；`.gitignore` 中 `.worktrees/` 重复一次（无害）。

---

## 五、建议修复优先级

| # | 项 | 严重度 | 位置 |
|---|---|---|---|
| 1 | authkey 不再进用户可见错误/日志（改掩码/长度） | High | `authorize.rs:1673,1941,1674` |
| 2 | 修正 `--verbose` 现实：提 dispatch/文件级别到 Trace，或改帮助文本 | High | `main.rs:24,416` |
| 3 | README/README_ZH 芯片表补 ESP32-P4/LN882H，`-d` 列表对齐 11 设备或链向 `docs/cli.md` | High | `README*.md:15-17,101` |
| 4 | 为 LN882H 加测试（`parse_hex_addr`/`resolve_segments`/4 KiB 对齐） | High | `plugins/ln882h/mod.rs` |
| 5 | auth 凭据不要明文持久化（或加密/不跨会话留存） | Medium | `flash-workspace.ts:43-44,222` |
| 6 | 设置 CSP（`default-src 'self'`） | Medium | `tauri.conf.json:30` |
| 7 | batch-flash-auth 接入 `usePortManagerStore` claim/release | Medium | `batch-flash-auth.ts` |
| 8 | 统一 workspace 加载校验（serial-debug/batch 对齐 flash-workspace 的严格解析） | Medium | `serial-debug-workspace.ts:52`、`batch-flash-auth-workspace.ts:48` |
| 9 | 收敛重复 crate 大版本（`thiserror→2`、`dirs→6`） | Medium | `Cargo.toml` × |
| 10 | CI 增加 Windows（至少）矩阵；clippy 覆盖 `tyutool_gui` | Medium | `.github/workflows/ci.yml` |
| 11 | `pickAutoSaveDir` 加 `isTauriRuntime()` 门控 | Medium | `serial-debug.ts:1033` |
| 12 | 硬编码中文串接入 i18n | Medium | `useFlashConnection.ts:149`、`ws-transport.ts:110,159` |
| 13 | 修 `listPorts()` 拆共享 WS 孤立 serial-debug | Medium | `ws-transport.ts:202` |
| 14 | AGENTS.md `.rs` 措辞改 snake_case；composable 命名加豁免条款 | Medium | `AGENTS.md:254` |
| 15 | 处置孤儿 `__test__/setup.ts`；合并 `settings.init.test.ts`；加 `coverage.thresholds` | Medium | stores/vitest.config.ts |

> 说明：本报告为只读审计产出，**未对源码做任何修改**。所有 `文件:行号` 均为各代理在 `refactor/v3` HEAD（v3.2.8）实际核对所得；其中 `phase-styles.ts:~39` 的行号为近似值，落地修复前建议复核。
