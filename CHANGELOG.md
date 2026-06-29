# Changelog

本项目所有重要变更记录于此 / All notable changes are documented here.

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
