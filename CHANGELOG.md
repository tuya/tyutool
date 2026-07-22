# Changelog

本项目所有重要变更记录于此 / All notable changes are documented here.

## [3.2.5] - 2026-07-22

### 新功能

- `batch-auth`：配置面板新增「授权」开关，关闭后批次仅烧录固件——不占用授权表 Excel，跳过授权与烧录后等待启动步骤，适用于只需烧录的产线场景

### 问题修复

- `esp`：烧录完成后通过串口协议触发 RTC 看门狗软复位，修复 TX/RX 直连治具（未接 DTR/RTS）上设备停留在下载模式、随后批量授权读不到数据的问题；原生 USB 端口与已接线的硬复位路径行为不变

---

### Features

- `batch-auth`: The config panel gains an Authorize on/off toggle — with it off, a batch flashes firmware only: no auth-sheet Excel is locked, and the auth step and post-flash boot wait are skipped, for flash-only production lines

### Bug Fixes

- `esp`: After flashing, the device is soft-reset by arming the RTC watchdog over the serial protocol itself, fixing TX/RX-only fixtures (no DTR/RTS wiring) where the chip stayed in download mode and the following batch authorize read nothing; native-USB ports and wired hard-reset paths are unchanged

## [3.2.4] - 2026-07-21

### 新功能

- `update`：应用内更新与 CLI 自更新的国内镜像源由 Gitee 迁移至 Tuya OSS；发布产物新增大陆版清单 `release.json`（下载地址指向 Tuya OSS），CLI 对应参数改为 `--source tuya`
- `release`：`latest.json` 与 `release.json` 的镜像字段调整为 `url_github` 与 `url_tuya`（移除 `url_gitee`）
- `batch-auth`：内置 T5AI 授权固件更新至 1.1.1
- `docs`：使用指南「固件烧录」页新增各标签页截图与波特率选择提示

### 问题修复

- `esp`：烧录/擦除/读取完成后自动硬复位设备退出下载模式，修复烧录后立即批量授权失败、需手动重新上电的问题
- `gui`：espflash 的日志级别上限设为 INFO，避免协议帧十六进制转储刷爆会话日志（单次烧录约 10 MB）
- `batch-auth`：新批次开始时清除上一轮残留的隔离标记；烧录/授权的终态不再残留上一台设备的 MAC、读取错误与授权凭证信息，避免污染界面状态与归档 CSV
- `batch-auth`：授权固件源类型与应用更新源解耦，应用更新源的调整不再影响授权固件下载

---

### Features

- `update`: The mainland-China mirror for in-app updates and CLI self-update moved from Gitee to Tuya OSS; releases now ship a China manifest `release.json` (download urls point at Tuya OSS), and the CLI flag is now `--source tuya`
- `release`: Mirror fields in `latest.json` and `release.json` are now `url_github` and `url_tuya` (`url_gitee` removed)
- `batch-auth`: Bundled T5AI auth firmware updated to 1.1.1
- `docs`: The usage-guide flash page gains per-tab screenshots and a baud-rate tip

### Bug Fixes

- `esp`: Hard-reset the device after flash/erase/read to exit download mode, fixing batch authorize failing right after flashing until a manual power-cycle
- `gui`: Cap espflash log output at INFO so protocol-frame hex dumps (~10 MB per flash) no longer flood the session log
- `batch-auth`: Starting a new batch clears the previous run's quarantine flag, and flash/auth terminal states no longer keep the previous device's MAC, read error, or credential info — preventing stale data in the UI and the archive CSV
- `batch-auth`: Auth-firmware sources are decoupled from the app-update sources, so update-source changes no longer affect auth-firmware downloads

## [3.2.3] - 2026-07-21

### 新功能

- `release`：`latest.json` 的每个下载条目新增 `url_gitee`（Gitee 镜像）与 `url_tuya`（Tuya OSS 镜像）字段，供外部系统按镜像源获取产物；应用内更新逻辑不变，仍使用 `url`
- `release`：同步到 Gitee 的 `latest.json` 现在与 GitHub Release 上的完全一致，不再单独生成 Gitee 专属版本

---

### Features

- `release`: Each download entry in `latest.json` now carries `url_gitee` (Gitee mirror) and `url_tuya` (Tuya OSS mirror) fields so external systems can fetch artifacts from a mirror; in-app updaters are unchanged and keep using `url`
- `release`: The `latest.json` synced to Gitee is now byte-identical to the one on the GitHub Release instead of a separately generated Gitee-specific variant

## [3.2.2] - 2026-07-17

### 新功能

- `batch-auth`：完成批次可一键归档——将授权表副本、固件（记录 SHA-256）、日志压缩包和批次摘要/槽位明细导出到带时间戳的文件夹
- `batch-auth`：剩余数量为 0 时仍可启动批次进行恢复核对，表格中已记录的设备按 MAC 重新匹配，支持 KV 丢失或写入中断后的补录
- `batch-auth`：OTP 授权改为「一次性写入」语义，移除多余的 eFuse 锁定及锁后重启校验步骤；旧版本 Excel 中的 OTPLOCKED 行仍兼容解析
- `batch-auth`：新增 ESP32 授权固件（1.0.0），T5AI 授权固件更新至最新构建
- `settings`：「关于」页新增「重新显示批量授权风险提示」按钮，无需开发者工具即可恢复已关闭的提示
- `settings`：更新说明现在只显示当前界面语言对应的内容
- `log`：全面加强诊断日志——串口开关/复位、烧录与授权关键节点、批量授权槽位生命周期、Excel 行写入审计、IPC 失败和 CLI 子命令现在都会写入日志文件，便于问题排查

### 问题修复

- `batch-auth`：加固 OTP 写一次安全——Excel 台账原子写入并滚动备份、装载时校验 UUID/AuthKey 长度（非法行标红并排除分配）、写入失败但设备已持有目标凭据时判定为成功；OTP 模式下冲突策略强制为「跳过」
- `batch-auth`：批次运行期间独占锁定 Excel 文件，写入失败会明确提示；批次结束后释放锁并重新读取文件，避免内存旧数据覆盖手工修改
- `batch-auth`：默认固件列表现在按芯片区分，切换芯片不再残留上一芯片的固件版本或下载错误芯片的固件
- `batch-auth`：授权完成/跳过后徽标状态正确更新，跳过行显示设备已有的 UUID，成功行不再误显示「未授权」
- `auth`：空 KV/OTP 设备回显的 "Authorization read failure." 现在按「未授权」处理，不再误报无效字符错误或在预检中浪费重试
- `esp`：原生 USB（USB-Serial-JTAG，如 ESP32-P4）端口现在强制使用 USB 复位序列，连接失败时给出可操作的下载模式指引
- `settings`：默认日志等级改为 debug 并在启动时同步到后端，确保排查所需的串口诊断日志默认写入文件

---

### Features

- `batch-auth`: Completed batches can be archived in one click — exports the auth-sheet copy, firmware binary (SHA-256 recorded), zipped logs, and batch summary/slot details into a timestamped folder
- `batch-auth`: Batches can now start at remaining = 0 for recovery verification — devices already recorded in the sheet are re-matched by MAC, supporting resume after KV loss or interrupted writes
- `batch-auth`: OTP authorization now uses write-once semantics, removing the redundant eFuse lock and post-lock reboot/verify steps; OTPLOCKED rows in Excel files from older builds still parse
- `batch-auth`: Add ESP32 auth firmware (1.0.0) and update the T5AI auth firmware to the latest build
- `settings`: Add a "show batch-auth risk notice again" button in About so the dismissed notice can be restored without DevTools
- `settings`: Release notes now show only the content matching the active UI language
- `log`: Strengthen diagnostic logging across the board — serial open/close/reset, flash and authorize milestones, batch-auth slot lifecycle, Excel row-write audit trail, IPC failures, and CLI subcommands are now recorded in the log file for troubleshooting

### Bug Fixes

- `batch-auth`: Harden write-once OTP safety — atomic Excel ledger writes with rolling backups, UUID/AuthKey length validation on load (invalid rows flagged red and excluded from allocation), and write failures where the device already holds the target credentials are treated as success; OTP mode forces the conflict policy to Skip
- `batch-auth`: Hold an exclusive lock on the Excel file while a batch runs and surface write failures clearly; the lock is released after the batch so the sheet is editable and re-read, preventing stale in-memory rows from clobbering manual edits
- `batch-auth`: The default firmware list is now per-chip, so switching chips no longer leaves the previous chip's versions in the dropdown or downloads the wrong chip's binary
- `batch-auth`: Auth badges update correctly after done/skipped — skipped rows show the device's existing UUID, and successful rows can no longer read "not authorized"
- `auth`: The "Authorization read failure." echo from empty KV/OTP devices is now treated as unauthorized instead of raising a bogus invalid-characters error or burning precheck retries
- `esp`: Native-USB ports (USB-Serial-JTAG, e.g. ESP32-P4) now force the USB reset sequence, and connect failures give actionable download-mode guidance
- `settings`: Default log level is now debug and is synced to the backend at startup, so the serial diagnostics needed for troubleshooting are written to the log file by default

## [3.2.1] - 2026-07-09

### 新功能

- `settings`：自动检查更新现在可配置间隔（关闭 / 1 小时 / 6 小时 / 12 小时 / 24 小时），且仅在距上次成功检查超过所选间隔后才静默执行；手动「检查更新」始终立即触发
- `settings`：刷新更新中心，并为每个更新来源提供独立的操作入口，便于逐源检查与安装
- `settings`：日志查看器新增「打开方式」，可在系统编辑器中打开当前日志文件

### 问题修复

- `serial-debug`：重启目标对话框现在会规范并校验芯片 ID，排除非芯片 ID 的值，避免重启到无效目标

---

### Features

- `settings`: Auto update checks are now interval-based (off / 1h / 6h / 12h / 24h) and run silently only after the selected interval has elapsed since the last successful check; manual “Check for updates” always runs immediately
- `settings`: Refresh the update center and add per-source update actions so each update source can be checked and installed independently
- `settings`: Add an “Open with” action in the log viewer to open the current log file in a system editor

### Bug Fixes

- `serial-debug`: Normalize and validate chip IDs in the reboot target dialog so non-chip values are excluded and the device no longer reboots to an invalid target

## [3.2.0] - 2026-07-08

### 新功能

- `serial-port-indicators`：新增串口活动指示器，并支持在设置中开关；侧边栏、工具箱入口和批量授权页现在都能直观看到各功能的串口占用状态

### 问题修复

- `firmware-flash`：烧录任务现在可以抢占串口调试占用的端口，减少必须手动先断开调试连接的情况
- `serial-debug`：等待异步串口交接完成后再开始烧录，修复功能切换时端口所有权竞争导致的连接失败
- `authorize`：授权预检阶段的 `auth-read` 现在会容忍无效响应并继续流程，提升脏串口环境下的兼容性

---

### Features

- `serial-port-indicators`: Add serial port activity indicators with a settings toggle so the sidebar, toolbox entry, and batch auth page can show per-feature port activity at a glance

### Bug Fixes

- `firmware-flash`: Let flash jobs preempt ports held by serial debug so users no longer need to manually disconnect first in common cases
- `serial-debug`: Wait for async port handoff to finish before flashing to fix connection failures caused by port-ownership races during feature handoff
- `authorize`: Tolerate invalid `auth-read` responses during authorize precheck so noisy serial lines no longer fail the flow prematurely

## [3.1.5] - 2026-07-07

### 新功能

- `batch-auth`：新增“烧录固件”开关，可对支持烧录的芯片直接切换为“仅授权”批次，并持久化这项共享配置
- `serial-debug`：芯片过滤结果支持回看更早匹配并导出完整会话/过滤结果，避免长时间会话只导出当前可见窗口

### 问题修复

- `serial-debug`：限制实时日志窗口和自动保存批次大小，修复清空会话、关闭连接与 WebSocket 桥接之间的竞态，并避免会话文件 ID 冲突
- `authorize`：`auth-read` 现在会拒绝乱码的 UUID/AuthKey 响应，避免串口脏数据被误判为有效授权信息
- `firmware-flash`：移除 Beken 传输层噪声日志，减少开发诊断时的无效刷屏
- `settings`：提升日志查看器“当前会话”徽标和截断提示的对比度

### 工程改进

- `serial-debug`：补充串口调试压力测试脚本与说明文档，便于验证长时间会话和筛选分页行为

---

### Features

- `batch-auth`: Add a Flash Firmware toggle so flash-capable chips can switch directly into auth-only batches, with the shared setting persisted
- `serial-debug`: Let chip-filtered logs load older matches and export the full session or filter result instead of only the currently visible window

### Bug Fixes

- `serial-debug`: Bound live log memory and autosave batches, fix clear-session / shutdown / WebSocket bridge races, and avoid session file ID collisions
- `authorize`: Reject garbled UUID/AuthKey payloads from `auth-read` so noisy serial data is not mistaken for valid credentials
- `firmware-flash`: Remove noisy Beken transport logs to reduce diagnostic spam during firmware flashing
- `settings`: Improve contrast for the log viewer's current-session badge and truncation notice

### Engineering

- `serial-debug`: Add a serial-debug stress script and documentation to validate long-running sessions and filter pagination

## [3.1.4] - 2026-07-01

### 新功能

- `log`：记录固件文件和 Excel 文件的选择（路径、大小），便于问题排查
- `log`：记录下载授权固件的失败原因及耗时，记录每个来源（URL）的失败详情
- `log`：记录 Excel 文件基本信息（行数、列等）和批量烧录启动参数
- `log`：`auth-write` 全部重试失败后，自动追加诊断性 `auth-read`，记录设备实际状态

### 问题修复

- `log`：将 HTTP/网络错误从 error 降级为 warn（请求可重试，非终态错误）
- `log`：将 firstByte RTT 日志从 info 降级为 debug（非用户可操作信息）

### 工程改进

- `ci`：前端仅构建一次，产物在所有 GUI 平台任务间复用，缩短 CI 时间
- `ci`：增强 macOS 代码签名流程

---

### Features

- `log`: Log firmware and Excel file selections (path, size) for easier troubleshooting
- `log`: Log auth-firmware download failure reason and elapsed time; record per-source (URL) failure details
- `log`: Log Excel file metadata (row count, columns, etc.) and batch-flash job parameters on start
- `log`: Append a diagnostic `auth-read` after all `auth-write` retries are exhausted to capture the device's actual state

### Bug Fixes

- `log`: Downgrade HTTP/network fetch errors from error to warn (retriable, not terminal)
- `log`: Downgrade firstByte RTT log from info to debug (not user-actionable)

### Engineering

- `ci`: Build frontend once and share the dist artifact across all GUI platform jobs to cut CI time
- `ci`: Enhance macOS code-signing process in release workflow

## [3.1.3] - 2026-06-30

### 问题修复

- `batch-auth`：批次完成后重新刷新 Excel 统计（已用 / 剩余），新增「占用中」计数显示失败槽位占用但未完成的行，确保总数始终对齐
- `batch-auth`：修复适配器未拔插时 Done 槽位导致 Start All 永久灰置、无法开始下一批次的问题；Done 槽位现与 Failed / no_code 一同在重新运行时重置为 Idle
- `batch-auth`：简化 `canStart` 逻辑，改为检查「无活跃槽位」而非枚举合法状态，确保批次部分完成时（如 3 Done 1 Running）按钮维持禁用
- `batch-auth`：修复 Start 按钮 tooltip 在可点击状态下仍显示、两个槽位同时完成时 `checkBatchCompletion` 双重触发的问题；批次进行中增加独立 tooltip 提示

## [3.1.2] - 2026-06-30

### 新功能

- `authorize`：OTP 锁成功后自动重启设备并二次验证（`auth-read`），确认 eFuse 锁定在重启后仍然生效
- `authorize`：`auth-otp-lock` 超时从 30s 延长至 60s，适配更慢的 eFuse 烧写硬件
- `authorize`：修复 OTP 锁失败时误触发硬件重置的问题

## [3.1.1] - 2026-06-29

### 问题修复

- `authorize`：将 `auth-otp-lock` idle 超时从 500ms 延长至 30s，修复慢速 eFuse 设备授权中途断开的问题
- `authorize`：`auth-read` 在 OTP 存储模式下使用独立 30s idle 超时，避免慢速设备读取被提前终止
- `ci`：auth-firmware 发布标记为预发布版本，防止意外抢占 Latest 标签

## [3.1.0] - 2026-06-29

### 新功能

**批量授权 (batch-auth)**

- MAC 优先流程：先读取设备 MAC，再按 MAC 匹配 Excel 行，支持每步写入 Excel 状态
- 行状态扩展为 6 状态机（Pending / Running / Done / Skipped / LockFailed / CancelledAfterWrite），每步完成后实时更新 Excel
- `auth_write` 失败自动重试最多 3 次（同时支持新旧固件写法）
- 重新运行时自动补充 OTP 锁（已写入授权但未锁的设备）
- 首次进入页面展示免责声明弹窗
- 支持 T5AI 默认 MAC（`C8:47:8C:00:00:18`）检测，读到默认值时报错停止
- eFuse 锁开关 UI（可选择是否写入 OTP 锁）
- `CancelledAfterWrite` 隔离区：授权写入后取消的设备单独显示，不参与重试
- `LockFailed` 槽位独立高亮，且排除在重试之外
- 加载 Excel 时检测并警告重复 MAC 绑定

**日志系统 (logging)**

- 每次启动生成独立会话日志文件（`tyutool-<timestamp>.log`），10 MB 上限后自动滚动（`tyutool-<ts>-N.log`）
- 日志查看器新增：文件列表导航、日志行着色（level）、关键字搜索

### 问题修复

- `batch-auth`：Skip 时回收已有 UUID 行，防止产生孤立条目
- `batch-auth`：Done 时保留最后一步的 Excel 记录，去掉多余的"完成"写入
- `authorize`：延长 OTP 命令超时，适配慢速 eFuse 烧写设备
- `tauri`：Excel 校验使用稳定错误码，前端可靠判断
- `log-viewer`：修复文件列表排序（按时间降序 + 名称次级排序）
- 若干 clippy 警告修复
