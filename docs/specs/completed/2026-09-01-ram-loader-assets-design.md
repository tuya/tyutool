# RAM 引导固件改为按需下载的发布资产

**日期：** 2026-09-01
**状态：** 已实现
**范围：** `tyutool-core`（新增 `ram_loader`）、`plugins/ln882h`、`plugins/gd32`、
`assets/ram-loader/`、`scripts/`、`.github/workflows/release-ram-loader.yml`
**计划：** `docs/plans/completed/2026-09-01-ram-loader-assets.md`
**参照：** `auth-firmware` 资产链路（`assets/auth-firmware/`、
`.github/workflows/release-auth-firmware.yml`）

---

## 问题

两颗芯片的 mask ROM 除了「写 SRAM 并跳转」几乎什么都做不了，刷写必须先上传一份厂商
下载器：

| 芯片 | 文件 | 大小 | 上传方式 |
|---|---|---|---|
| LN882H | `plugins/ln882h/ram.bin` | 37 872 B | XMODEM 到 `0x20000000` |
| GD32VW553 | `plugins/gd32/loader.bin` | 15 600 B | AN3155 ROM bootloader 到 `0x20002000` |

两者都是 `include_bytes!` 编进 `tyutool-core`，也就是编进 CLI、GUI 和 bridge 三个二进制。
**不希望厂商固件继续留在工具和代码里**，改为像 `auth-firmware` 那样作为资产按需下载。

## 决定

### 1. 目录与命名：完全复用 auth-firmware 的形状

```
assets/ram-loader/<chip>/ram-loader-<chip>-<version>.bin   # 发布后不可变
assets/ram-loader/<chip>/ram-loader-<chip>-<version>.txt   # 可选 notes（厂商溯源信息）
```

一级目录名是小写 `ChipId`（`ln882h` / `gd32vw553`，与 `chip-manifests.ts` 对齐）；违规由生成
脚本报错、卡住 workflow。发布端（GitHub Release / Gitee Release / 涂鸦 CDN）一律平铺，
URL 规则统一为 `<base>/<filename>`，所以一个生成器换三次 `BASE_URL` 即可。

bin **继续留在 git 里**：它是 CI 的发布源，也是唯一可追溯的源头，和 auth-firmware 一致。
"不在工具里"指的是不再编进二进制，不是不在仓库里。

### 2. 版本号只能自己定

翻了两个固件的字节：

- GD32 有厂商身份但**没有版本号**——`SDK release version:` 后面是空的，只有
  `SDK build revision: 94fb25571b15fbea` 和 `SDK build date: 2025/07/04 10:29:16`。
- LN882H **什么都没有**。它的 `version` 控制台命令回复 `RAMCODE`，那是模式探测而非版本；
  镜像里只有 LN SDK 构建树的断言路径。

所以 `<version>` 是 tyutool 自己的资产版本，从 `1.0.0` 起单调递增，厂商溯源写进同名 `.txt`
（进 manifest 的 `notes`）。用 `94fb25571b15fbea` 当版本号被否掉：LN882H 没有对应物，且不
可排序。

### 3. 版本在代码里钉死，不"取最新"

插件声明它写代码时对着的那一份，含摘要：

```rust
const LOADER: RamLoaderRef = RamLoaderRef {
    chip: "gd32vw553", version: "1.0.0", size: 15_600,
    sha256: "2559d822553f2af8f9f4ff26201fffac151f8e13ec29b6b1c0241215445d373e",
};
```

`ram_loader::resolve` 只找这一条，并**按代码里的摘要校验**，manifest 自带的 `sha256` 仅作
交叉检查（不一致时记一行 warn）。

**否掉"取该芯片最新版"**：固件搬出二进制后，工具对资产源的信任度决定了设备安全。钉死摘要
意味着 manifest 被篡改或误发布都喂不进别的 loader，也意味着发新 loader 不会影响已发布的
旧版工具。代价是换 loader 需要改一行常量并发版——这正是想要的门槛。

### 4. 解析顺序与逃生口

1. `TYUTOOL_RAM_LOADER_DIR`（若设置）——只读本地目录，绝不联网，产线/内网机器用。
   同时接受平铺和 `<chip>/` 两种摆法。
2. 缓存 `<cache_dir>/com.tyutool.shared/ram-loader/<chip>/<file>`——CLI / GUI / bridge 共用一份，
   （本文实现时写的是 `<cache_dir>/tyutool/ram-loader`，次日被
   `2026-09-02-user-dir-layout-design.md` 的统一命名取代），
   一次下载三端复用。
3. 下载（`download` feature，三个镜像依次尝试）。

**文件存在但校验失败是硬错误，不是 cache miss**：静默重下会掩盖操作员放错文件这件事。
唯一例外是缓存自己损坏——那是我们写的，记 warn 后重下。

解析发生在**开串口之前**：拿不到 loader 的任务应该在设备还没被碰过时就失败。

### 5. HTTP 放 core，藏在 feature 后面

`ram_loader` 整条流程（manifest 解析、缓存、校验、下载）都在 `tyutool-core`——三个二进制
都要用，按 crate 边界规则不属于任何前端。HTTP 依赖藏在新 feature `download`（reqwest
blocking + rustls）后面，由 tyutool-cli / src-tauri / tyutool-bridge 打开，和 `zip` / `excel`
一个套路。关掉时 `resolve` 退化为「缓存 + 环境变量」，这正是 `tyutool-serve` 和测试想要的
形状（serve 的唯一宿主 tyutool-cli 已经打开了这个 feature）。

**否掉「runtime 直接拼 URL、不要 manifest」**：少一次请求，但镜像路径一旦变动，已发布的旧版
工具就变砖。固件不再内嵌之后，资产可达性就是可用性，manifest 这层间接换来的是「镜像可以搬家
而不必发版」。

### 6. manifest 生成器抽公共层，不复制

`auth-firmware` 与 `ram-loader` 的扫描、命名校验、哈希、排序完全一样，抽到
`scripts/lib/firmware-asset-manifest.ts`，两个家族各留一个瘦入口；Gitee 发布脚本改名为
`scripts/publish-firmware-assets-gitee.sh` 并全 env 参数化（`ASSET_DIR` / `TAG` /
`MANIFEST_FILE` / `RELEASE_NAME` / `RELEASE_BODY`）。auth-firmware 的对外行为（导出名、
manifest 形状、`other/` 禁用）保持不变，其 23 个测试原样通过。

### 7. 用户可见事件

新增 `FlashPhase::FetchRamLoader`，只在真的要下载时发一次（命中缓存不发）。不复用
`Percent`——loader 只有几十 KB，混进烧录进度条只会让人误解。前端手写镜像
（`flash-ipc-types.ts`）、batch-flash-auth 的相位标签和两份 i18n 一并补上。

## 测试怎么保证不联网

- `ram_loader::repo_asset_bytes`（`#[cfg(test)]`）读 `assets/ram-loader/` 里的文件并按钉死的
  摘要校验，两个插件各一条测试。这是 gd32 原来那条 `the_bundled_loader_is_the_captured_one`
  的去处——固件不再内嵌之后，**它是唯一能拦住「常量和即将发布的文件不一致」的东西**。
- 流程测试改为喂合成 loader 字节（gd32 的 `fake_loader()`），`flash_with` / `erase_with` /
  `bring_up_loader` 因此多一个 `loader: &[u8]` 参数。
- `ram_loader` 自己的单测覆盖校验、两种目录摆法、缓存写入（含不留 tmp 文件）、manifest 选条
  与报错文案，全部走临时目录。**没有测试读写进程环境变量**：`resolve` 里的 env 读取只有三行，
  而在同一测试二进制里改 env 会与并发测试互相污染。

## 发布 CI

`.github/workflows/release-ram-loader.yml`，形状照 `release-auth-firmware.yml`：
`workflow_dispatch` + 串行 concurrency（不取消在途上传）→ 生成 manifest → 发到固定 tag
`ram-loader`（bin 已存在则跳过，`ram-loader.json` 覆盖）→ Gitee 同步 → 额外产出一个
`ram-loader-cdn` artifact（含按 CDN base 生成的 `ram-loader.json` 和全部 bin），供手工上传涂鸦
CDN 时原样铺进目录。

## 明确不做

| 想法 | 为什么不做 |
|---|---|
| bin 移出 git，workflow 上传时手动给文件 | 失去可追溯性和 CI 幂等性；auth-firmware 已经证明留在仓库可行 |
| 取该芯片最新版本 | 见 §3，把设备安全押在资产源上 |
| runtime 直接拼 URL，不发 manifest | 见 §5，镜像搬家会让旧版工具变砖 |
| CDN 目录按 chip 分层 | 三个镜像共用一条 URL 规则才能共用一个生成器 |
| `tyutool_cli ram-loader fetch` 预热子命令 | 目前没有场景要求预热而不刷机；真需要时再加，并同步 `docs/cli.md` |
| 下载进度事件（`Percent`） | 37 KB 的下载，进度条只会闪一下 |
