# RAM 引导固件资产化 实施计划

**Spec:** `docs/specs/completed/2026-09-01-ram-loader-assets-design.md`

**状态：** 已交付。

**Goal:** 把 LN882H / GD32VW553 的 RAM 引导固件从 `include_bytes!` 改成按需下载并缓存的发布
资产，工具二进制里不再携带厂商固件；同时补上 GitHub / Gitee 的自动发布 CI。

**Architecture:** 新增 `tyutool_core::ram_loader` 一个模块承载全部流程（环境变量目录 → 缓存 →
下载，全程按插件钉死的摘要校验）；HTTP 藏在 core 的新 feature `download` 后面。资产目录、
manifest 生成器和 Gitee 发布脚本与既有 `auth-firmware` 家族共用，抽出公共层而不复制。

**Tech Stack:** Rust 2021（sha2、dirs、可选 reqwest blocking + rustls）、TypeScript（tsx +
vitest）、GitHub Actions。

**基线（`0546664`）：** core 365（含 download feature）/ cli / serve / tauri / bridge 全绿。

---

## File Map

| File | Change |
|---|---|
| `assets/ram-loader/README.md` | 新增：命名规则、版本约定、双半发布流程 |
| `assets/ram-loader/{ln882h,gd32vw553}/ram-loader-*-1.0.0.bin` | 从 `plugins/` 迁入（`git mv`） |
| `assets/ram-loader/{ln882h,gd32vw553}/ram-loader-*-1.0.0.txt` | 新增：厂商溯源 notes |
| `crates/tyutool-core/src/ram_loader.rs` | 新增：`RamLoaderRef` / `resolve` / 缓存 / 校验 / manifest |
| `crates/tyutool-core/src/lib.rs` | 注册 `pub mod ram_loader` |
| `crates/tyutool-core/src/flash_event.rs` | 新增 `FlashPhase::FetchRamLoader` |
| `crates/tyutool-core/Cargo.toml` | 新 feature `download`；`dirs`、可选 `reqwest` |
| `crates/tyutool-core/src/plugins/ln882h/mod.rs` | `RAM_BIN` → `RAM_LOADER` 常量；`boot` 收参 |
| `crates/tyutool-core/src/plugins/gd32/mod.rs` | `LOADER_BIN` → `LOADER` 常量；三个函数收参；测试改喂合成字节 |
| `crates/tyutool-{cli,bridge}/Cargo.toml`、`src-tauri/Cargo.toml` | 打开 `download` |
| `crates/tyutool-cli/src/reporter.rs` | 新相位的标签 |
| `scripts/lib/firmware-asset-manifest.ts` | 新增：两个家族共用的扫描/校验/哈希/排序 |
| `scripts/generate-auth-firmware-manifest.ts` | 改为瘦入口，对外 API 不变 |
| `scripts/generate-ram-loader-manifest.ts`(+`.test.ts`) | 新增 |
| `scripts/publish-auth-firmware-gitee.sh` → `publish-firmware-assets-gitee.sh` | 改名 + 全 env 参数化 |
| `.github/workflows/release-ram-loader.yml` | 新增发布 workflow |
| `.github/workflows/release-auth-firmware.yml` | 跟进脚本改名与新 env |
| `.github/workflows/ci.yml` | feature 步骤加 `download` |
| `src/features/firmware-flash/flash-ipc-types.ts` | 手写镜像补 `fetch_ram_loader` |
| `src/features/batch-flash-auth/components/BatchFlashAuthSlotRow.vue`、`src/locales/*.json`、`src/i18n/index.test.ts` | 相位标签 |
| `docs/cli.md` | 新增「RAM loader downloads」小节 + 两条 Common errors |
| `AGENTS.md` | 新增「Published firmware assets」小节；架构树补 `assets/`；CI 门禁更新 |

---

## Task 1: 摸清厂商版本信息

- [x] **Step 1:** `strings` + 十六进制核对两个 bin → verify：GD32 只有 build revision/date，
  `SDK release version:` 为空；LN882H 无任何版本信息
- [x] **Step 2:** 定 `<version>` 为 tyutool 自有版本，从 `1.0.0` 起 → verify：写进 README 与
  两份 `.txt`

## Task 2: 资产目录

- [x] **Step 1:** `git mv` 两个 bin 进 `assets/ram-loader/<chip>/` → verify：git 识别为 rename
- [x] **Step 2:** 写 README + 两份 notes → verify：生成器能读出 `notes`

## Task 3: 生成器与发布脚本

- [x] **Step 1:** 抽 `scripts/lib/firmware-asset-manifest.ts`，auth 入口改瘦 → verify：
  auth-firmware 原有 23 个测试不改一行仍通过
- [x] **Step 2:** 新增 ram-loader 入口 + 4 个测试 → verify：`vitest run scripts/` 全绿
- [x] **Step 3:** 真实资产跑一遍生成器 → verify：输出的 sha256 与插件钉死的常量一致
- [x] **Step 4:** Gitee 脚本改名 + 参数化，auth workflow 跟进 → verify：默认值仍指向
  auth-firmware，行为不变

## Task 4: `ram_loader` 模块

- [x] **Step 1:** `RamLoaderRef` / 校验 / 两种目录摆法 / 缓存路径与原子写 → verify：单测覆盖，
  含「不留 tmp 文件」
- [x] **Step 2:** manifest 解析与选条（含缺版本、坏 JSON 的文案）→ verify：单测
- [x] **Step 3:** `download` feature 下的三镜像下载 → verify：`--features download` 的 clippy
  与测试
- [x] **Step 4:** 不可用时的用户文案（说清缺什么、怎么手工补）→ verify：单测断言含文件名与
  环境变量名

## Task 5: 两个插件改造

- [x] **Step 1:** gd32：常量替换，`bring_up_loader`/`flash_with`/`erase_with` 收 `loader: &[u8]`，
  在开串口前 resolve → verify：`cargo test -p tyutool-core` 全绿
- [x] **Step 2:** gd32 测试：`fake_loader()` 替代内嵌镜像；原摘要断言改为
  `repo_asset_bytes(&LOADER)` → verify：仍能拦住常量与资产不一致
- [x] **Step 3:** ln882h：同样改造，三个 run_* 各自 resolve → verify：新增
  `the_pinned_ram_loader_matches_the_published_asset`

## Task 6: 事件与前端

- [x] **Step 1:** `FlashPhase::FetchRamLoader` + CLI reporter 标签 → verify：非穷尽 match 编译
  错误已消除
- [x] **Step 2:** 前端手写镜像、batch-auth 相位表、两份 i18n、i18n 测试列表 → verify：
  `src/i18n/index.test.ts` 通过

## Task 7: CI 与文档

- [x] **Step 1:** `release-ram-loader.yml`（GitHub + Gitee + CDN artifact）
- [x] **Step 2:** `ci.yml` feature 步骤加 `download`，AGENTS.md 门禁清单同步
- [x] **Step 3:** `docs/cli.md`、`AGENTS.md`、本对文档

## Task 8: 全量门禁

- [x] `cargo fmt --all --check`
- [x] `cargo clippy` × 5 crates（含 `--features …,download`）
- [x] `cargo test` × 5 crates
- [x] `pnpm run lint && typecheck:scripts && test:coverage && build`

---

## 仍需人工的一步

发布 workflow 是 `workflow_dispatch`，**首次发布必须手动触发一次**（`Release ram-loader`）。
在那之前，只有本地缓存或 `TYUTOOL_RAM_LOADER_DIR` 能让这两颗芯片刷写成功——已发布的旧版本
工具不受影响（它们仍内嵌固件）。涂鸦 CDN 那份按 artifact 手工上传。
