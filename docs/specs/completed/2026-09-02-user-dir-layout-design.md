# 三个二进制的用户目录命名统一

**日期：** 2026-09-02
**状态：** 已实现
**范围：** `tyutool-core`（新增 `paths`）、`tyutool-cli`、`tyutool-bridge`、文档与 issue 模板
**计划：** `docs/plans/completed/2026-09-02-user-dir-layout.md`

---

## 问题

一个产品的三个二进制在磁盘上用了三套名字，同一类数据还落在不同类别的目录里。Windows 上的现状：

```
%APPDATA%\com.tyutool.desktop\    GUI 设置 / 工作区
%APPDATA%\tyutool\               CLI 日志          ← 日志不该进漫游配置
%APPDATA%\tyutool-bridge\        bridge 日志 + grants.json + autostart.json
%LOCALAPPDATA%\com.tyutool.desktop\  GUI 日志、auth 固件缓存、WebView profile
%LOCALAPPDATA%\tyutool\ram-loader\   共享缓存
%TEMP%\tyutool\serial-debug\
```

用户在文件管理器里看到三个不相干的名字，认不出它们属于同一个工具。

## 决定

### 1. 每个产品一个反向 DNS id，跨类别保持一致

| id | 归属 | 来源 |
|---|---|---|
| `com.tyutool.desktop` | GUI | Tauri `identifier`，真实 bundle id |
| `com.tyutool.cli` | CLI | 本次新铸——CLI 是裸二进制，没有自己的 bundle |
| `com.tyutool.bridge` | bridge | 它 packager 里已有的 identifier |
| `com.tyutool.shared` | 无单一归属的数据 | 本次新铸 |

### 2. 目录类别由「数据是什么」决定，不合并

三个系统都按数据类别切分用户目录，每一类有不同的系统行为：macOS 的 `Application Support`
进 Time Machine 备份、`Caches` 不备份且被视作可回收、`Logs` 被 Console.app 收录；Windows 的
Roaming 跟着账号漫游而 Local 不会；XDG 把 `XDG_CACHE_HOME` 定义为「非必要的缓存数据」。

所以**一个程序的文件分布在三个目录里是正确的**，把它们并成一个反而会出事：缓存进
Application Support 会被 Time Machine 反复备份，设置进 Caches 会被清理工具删掉且没有备份。
本次不做任何「集中到一个文件夹」的尝试；用户要找齐路径，靠文档，不靠目录结构。

**明确否掉了加一个 `tyutool paths` 子命令**——它只是把路径再打印一遍，`docs/cli.md` 和
issue 模板已经承担了这件事，不值得为此扩 CLI 表面。

### 3. `com.tyutool.desktop` 不能当作家族根

一度考虑把 CLI / bridge 收进 GUI 的 id 下（`com.tyutool.desktop/{gui,cli,bridge}`），因为
那个目录反正必须存在——Windows/Linux 上 Tauri 在建窗口前**强制**把 WebView profile 设成
`data_local_dir()/<identifier>`（`tauri/src/manager/webview.rs`），而本仓库没有覆盖它。

否掉的理由：macOS 的 AppCleaner / CleanMyMac 一类卸载工具**按 bundle id** 扫
`Application Support` / `Caches` / `Logs`，用户只卸载 GUI 就会连带删掉 CLI 和 bridge 的
数据——其中 `grants.json` 是凭证，丢了要重新授权。三个 id 各自独立就没有这个问题。

反过来也不做：把 GUI 搬到朴素的 `tyutool/` 根同样被否——WebView profile 那一坨搬不动
（要放弃声明式建窗，并迁移一个装着主题/语言/日志级别/免责声明的 LevelDB），结果会是
`%LOCALAPPDATA%` 下同时留着 `tyutool\` 和 `com.tyutool.desktop\`，比现状更乱。

### 4. GUI 一行不改

按上面两条规则检查，GUI 现有的三处（`~/Library/Logs/com.tyutool.desktop`、
`Application Support/com.tyutool.desktop`、`Caches/com.tyutool.desktop`）本来就合规。
需要规范的只有 CLI 和 bridge——它们把日志写进了数据目录——以及 core 的两个共享目录。

### 5. 迁移只覆盖会让用户重做工作的数据

日志是诊断数据，旧文件留在原处不搬（`logs --dir` 仍能读），文档里说明。
`grants.json` / `autostart.json` 是用户决策，首次启动移动一次：按文件名搬、不搬目录
（macOS/Windows 上 `config_dir()` 与 `data_dir()` 同一个目录，整目录搬会把旧日志一起带走）；
目标已存在的文件永远优先；失败只告警不致命——bridge 是常驻进程，为一次搬迁拒绝启动比重新
授权糟糕得多。

## 不做

| 想法 | 为什么 |
|---|---|
| 把三个产品的数据并进一个根 | AppCleaner 按 bundle id 扫，会连带删掉别的产品的凭证 |
| 把一个产品的三类数据并进一个目录 | 与三个系统的备份/回收语义打架，见 §2 |
| 把 GUI 搬到朴素的 `tyutool/` 根 | WebView profile 搬不动，结果是两个根并存 |
| 改 Tauri `identifier` | 动 bundle 身份、代码签名与更新链路，还会重置 WebView 里的全部设置 |
| 改 `AUTOSTART_APP_NAME` | 它是 LaunchAgent label / `.desktop` 文件名 / `HKCU\Run` 键名，改名会让现存自启注册变孤儿且不会自愈 |
| 合并两个二进制的日志目录 | `pick_active_log` 取「最新的 `*.log`」、`prune_log_files` 预算按目录算，会互相遮蔽和挤占 |
| 搬 GUI 的 `settings.json` / `.tyutool-workspace.dat` | 已在合规位置；两者命名不一致是另一个问题（`docs/audits/2026-08-04-code-audit.md:117`），单独排 |
| 加 `tyutool paths` 子命令 | 见 §2 |

## 顺带修掉的两个毛病

- CLI 日志从 Windows 的 Roaming 移到 Local。
- `default_log_dir()` 在 `data_dir()` 返回 `None` 时回退到 `PathBuf::from(".")`，也就是往**当前
  工作目录**写日志；改为回退到临时目录。

## 发版前必须实机验证

1. Windows 覆盖安装一次：NSIS 的升级识别走注册表 uninstall key，与数据目录无关，但没有一手
   证据，必须实测。
2. 升级后 bridge 的开机自启仍然生效、已授权的 origin 不需要重新确认（即
   `migrate_legacy_config_files` 真的搬到了）。
