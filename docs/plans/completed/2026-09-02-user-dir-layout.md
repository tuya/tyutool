# 用户目录命名统一 实施计划

**Spec:** `docs/specs/completed/2026-09-02-user-dir-layout-design.md`

**状态：** 已交付（待实机验证两项，见 spec 末尾）。

**Goal:** 三个二进制各用一个反向 DNS id，每类数据落在系统为该类保留的目录里；GUI 不动，
规范 CLI、bridge 与 core 的共享目录。

**Architecture:** 新增 `tyutool_core::paths` 承载 id 常量与四个类别的路径解析，
`log_dir` 逐字复刻 Tauri `app_log_dir()` 的公式，使 GUI（经 Tauri 解析）与另外两个落在同一形状。

**Tech Stack:** Rust 2021（`dirs`）。无新依赖。

**基线（`e63b922`）：** core 378 / cli 60 / serve 31 / gui 36 / bridge 45。

---

## File Map

| File | Change |
|---|---|
| `crates/tyutool-core/src/paths.rs` | 新增：四个 id 常量 + `log_dir` / `config_dir` / `cache_dir` / `temp_dir` |
| `crates/tyutool-core/src/lib.rs` | `pub mod paths` |
| `crates/tyutool-core/src/ram_loader.rs` | 缓存根改 `SHARED_ID` |
| `crates/tyutool-core/src/serial_debug.rs` | 归档根改 `SHARED_ID` |
| `crates/tyutool-cli/src/main.rs` | `default_log_dir` 改 `CLI_ID` 的日志目录；CWD 回退改临时目录 |
| `crates/tyutool-bridge/src/main.rs` | 会话日志改 `BRIDGE_ID` 的日志目录 |
| `crates/tyutool-bridge/src/lib.rs` | 新增 `config_dir()` + `migrate_legacy_config_files` + 两个测试 |
| `crates/tyutool-bridge/src/autostart.rs` | 走 `crate::config_dir()` |
| `docs/cli.md` | 日志目录表、RAM loader 缓存表、`logs --dir` 示例 |
| `.github/ISSUE_TEMPLATE/bug_report.yml` | CLI / bridge 日志路径、grants.json 警告路径 |
| `AGENTS.md` | 新增「Per-user file locations」；日志契约表与归档路径 |
| `crates/tyutool-bridge/{AGENTS.md,PROTOCOL.md}` | grants.json 路径与迁移说明 |
| `docs/specs/completed/2026-09-01-ram-loader-assets-design.md` | 标注缓存路径已被本次取代 |

---

## Task 1: 共享路径模块

- [x] **Step 1:** 从 `tauri/src/path/desktop.rs` 读出 `app_*_dir()` 的真实公式 → verify：
  `log_dir` 与之逐字一致（macOS `~/Library/Logs/<id>`，其他 `data_local_dir()/<id>/logs`）
- [x] **Step 2:** 四个 id 常量 + 四个解析函数 → verify：单测断言 id 形状、互异、各类别路径含 id
- [x] **Step 3:** Windows 专属测试：日志目录不在 Roaming 下 → verify：`#[cfg(windows)]` 测试

## Task 2: core 的两个共享目录

- [x] **Step 1:** `ram_loader::cache_dir` → `SHARED_ID`
- [x] **Step 2:** `serial_debug_archive_dir` → `SHARED_ID` → verify：`cargo test -p tyutool-core` 全绿

## Task 3: CLI

- [x] **Step 1:** `default_log_dir` 改用 `paths::log_dir(CLI_ID)`，回退改临时目录
- [x] **Step 2:** 旧测试断言目录名叫 `tyutool` → 改为断言绝对路径、含 `CLI_ID`、且与共享解析器一致

## Task 4: bridge

- [x] **Step 1:** 会话日志改 `paths::log_dir(BRIDGE_ID)`
- [x] **Step 2:** `config_dir()` 集中到一处，grants 与 autostart 共用
- [x] **Step 3:** `migrate_legacy_config_files`：按文件名搬、目标已存在则不覆盖、失败只告警
  → verify：两个测试（含「旧日志必须留在原处」「重复执行不变」「无旧目录时不创建任何东西」）

## Task 5: 文档

- [x] `docs/cli.md`、issue 模板、根 `AGENTS.md`、bridge 的 `AGENTS.md` / `PROTOCOL.md`
- [x] 本对文档；并给前一天的 ram-loader spec 标注路径已被取代

## Task 6: 门禁

- [x] `cargo fmt --all --check`
- [x] clippy × 5 crates（含 `--features …,download`）
- [x] test × 5 crates
- [x] `pnpm lint / typecheck:scripts / test:coverage / build`

---

## 交付后仍需人工

见 spec 末尾两项实机验证：Windows 覆盖安装、升级后 bridge 自启与授权不丢。
