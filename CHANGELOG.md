# Changelog

本项目所有重要变更记录于此 / All notable changes are documented here.

## [3.1.4] - 2026-07-01

### 新功能 / Features

- `log`：记录固件文件和 Excel 文件的选择（路径、大小），便于问题排查 / Log firmware and Excel file selections (path, size) for easier troubleshooting
- `log`：记录下载授权固件的失败原因及耗时，记录每个来源（URL）的失败详情 / Log auth-firmware download failure reason and elapsed time; record per-source (URL) failure details
- `log`：记录 Excel 文件基本信息（行数、列等）和批量烧录启动参数 / Log Excel file metadata (row count, columns, etc.) and batch-flash job parameters on start
- `log`：`auth-write` 全部重试失败后，自动追加诊断性 `auth-read`，记录设备实际状态 / Append a diagnostic `auth-read` after all `auth-write` retries are exhausted to capture the device's actual state

### 问题修复 / Bug Fixes

- `log`：将 HTTP/网络错误从 error 降级为 warn（请求可重试，非终态错误） / Downgrade HTTP/network fetch errors from error to warn (retriable, not terminal)
- `log`：将 firstByte RTT 日志从 info 降级为 debug（非用户可操作信息） / Downgrade firstByte RTT log from info to debug (not user-actionable)

### 工程改进 / Engineering

- `ci`：前端仅构建一次，产物在所有 GUI 平台任务间复用，缩短 CI 时间 / Build frontend once and share the dist artifact across all GUI platform jobs to cut CI time
- `ci`：增强 macOS 代码签名流程 / Enhance macOS code-signing process in release workflow

## [3.1.3] - 2026-06-30

### 问题修复 / Bug Fixes

- `batch-auth`：批次完成后重新刷新 Excel 统计（已用 / 剩余），新增「占用中」计数显示失败槽位占用但未完成的行，确保总数始终对齐
- `batch-auth`：修复适配器未拔插时 Done 槽位导致 Start All 永久灰置、无法开始下一批次的问题；Done 槽位现与 Failed / no_code 一同在重新运行时重置为 Idle
- `batch-auth`：简化 `canStart` 逻辑，改为检查「无活跃槽位」而非枚举合法状态，确保批次部分完成时（如 3 Done 1 Running）按钮维持禁用
- `batch-auth`：修复 Start 按钮 tooltip 在可点击状态下仍显示、两个槽位同时完成时 `checkBatchCompletion` 双重触发的问题；批次进行中增加独立 tooltip 提示

## [3.1.2] - 2026-06-30

### 新功能 / Features

- `authorize`：OTP 锁成功后自动重启设备并二次验证（`auth-read`），确认 eFuse 锁定在重启后仍然生效
- `authorize`：`auth-otp-lock` 超时从 30s 延长至 60s，适配更慢的 eFuse 烧写硬件
- `authorize`：修复 OTP 锁失败时误触发硬件重置的问题

## [3.1.1] - 2026-06-29

### 问题修复 / Bug Fixes

- `authorize`：将 `auth-otp-lock` idle 超时从 500ms 延长至 30s，修复慢速 eFuse 设备授权中途断开的问题
- `authorize`：`auth-read` 在 OTP 存储模式下使用独立 30s idle 超时，避免慢速设备读取被提前终止
- `ci`：auth-firmware 发布标记为预发布版本，防止意外抢占 Latest 标签

## [3.1.0] - 2026-06-29

### 新功能 / Features

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

### 问题修复 / Bug Fixes

- `batch-auth`：Skip 时回收已有 UUID 行，防止产生孤立条目
- `batch-auth`：Done 时保留最后一步的 Excel 记录，去掉多余的"完成"写入
- `authorize`：延长 OTP 命令超时，适配慢速 eFuse 烧写设备
- `tauri`：Excel 校验使用稳定错误码，前端可靠判断
- `log-viewer`：修复文件列表排序（按时间降序 + 名称次级排序）
- 若干 clippy 警告修复
