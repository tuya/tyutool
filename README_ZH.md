# tyutool

[![Release](https://img.shields.io/github/v/release/tuya/tyutool?style=flat-square)](https://github.com/tuya/tyutool/releases/latest)
[![License](https://img.shields.io/github/license/tuya/tyutool?style=flat-square)](LICENSE.txt)
[![Platform](https://img.shields.io/badge/平台-Linux%20%7C%20macOS%20%7C%20Windows-blue?style=flat-square)](https://github.com/tuya/tyutool/releases/latest)

涂鸦 IoT 设备固件烧录工具，提供跨平台桌面 GUI（Tauri 2 + Vue 3）和独立命令行（CLI）两种使用方式。

## 支持芯片

| 芯片系列 | 型号 |
|---------|------|
| Tuya    | T1、T2、T3、T5 |
| Beken   | BK7231N |
| Espressif | ESP32、ESP32-C3、ESP32-C6、ESP32-S3 |

## 下载

从 [GitHub Releases](https://github.com/tuya/tyutool/releases/latest) 或 [Gitee 镜像](https://gitee.com/tuya-open/tyutool/releases) 获取最新版本。

### GUI 桌面版

| 平台 | 安装包 |
|------|--------|
| Linux x86\_64 | `.AppImage`、`.deb`、`.rpm`、便携版 tar.gz |
| Linux aarch64 | `.AppImage`、`.deb`、便携版 tar.gz |
| macOS（Universal） | `.dmg`、便携版 tar.gz |
| Windows x86\_64 | NSIS 安装包（`.exe`）、便携版 `.zip` |

**各平台推荐下载**（文件名中 `x.x.x` 为版本号，从 [Releases](https://github.com/tuya/tyutool/releases/latest) 获取最新版）

| 平台 | 推荐文件 | 自动更新 | 备注 |
|------|----------|:--------:|------|
| Windows x86\_64 | ★ [`tyutool-gui_windows_x86_64_nsis_x.x.x.exe`](https://github.com/tuya/tyutool/releases/latest) | ✅ | NSIS 安装包 |
| Windows x86\_64 | [`tyutool-gui_windows_x86_64_portable_x.x.x.zip`](https://github.com/tuya/tyutool/releases/latest) | ❌ | 免安装便携版 |
| macOS Universal | ★ [`tyutool-gui_macos_universal_dmg_x.x.x.dmg`](https://github.com/tuya/tyutool/releases/latest) | ✅ | DMG 安装包 |
| macOS Universal | [`tyutool-gui_macos_universal_portable_x.x.x.tar.gz`](https://github.com/tuya/tyutool/releases/latest) | ❌ | 解压即用便携版 |
| Linux x86\_64 | ★ [`tyutool-gui_linux_x86_64_appimage_x.x.x.AppImage`](https://github.com/tuya/tyutool/releases/latest) | ✅ | `chmod +x` 后运行，跨发行版 |
| Linux aarch64 | ★ [`tyutool-gui_linux_aarch64_appimage_x.x.x.AppImage`](https://github.com/tuya/tyutool/releases/latest) | ✅ | `chmod +x` 后运行，跨发行版 |
| Linux x86\_64 | `tyutool-gui_linux_x86_64_deb_x.x.x.deb` / `_rpm_x.x.x.rpm` | ❌ | Debian 系 / Fedora·RHEL 系 |
| Linux aarch64 | `tyutool-gui_linux_aarch64_deb_x.x.x.deb` | ❌ | Debian 系 |
| Linux x86\_64 / aarch64 | `tyutool-gui_linux_*_portable_x.x.x.tar.gz` | ❌ | 解压后运行目录内程序 |

**常见问题**

| 问题 | 平台 | 处理方式 |
|------|------|----------|
| 「无法验证开发者」或被 Gatekeeper 拦截 | macOS | 安装包未做 Apple 代码签名，属正常安全策略。**系统设置 → 隐私与安全性 → 仍要打开**；或在 Finder 中右键 `tyutool.app` → **打开** |
| 串口不出现 | macOS | **系统设置 → 隐私与安全性 → 配件**（或「允许配件连接」等，文案随系统版本而异） |
| 窗口全白 / 空白（虚拟机常见） | Linux | WebKit2GTK GPU 合成失败所致，启动前设置环境变量：`WEBKIT_DISABLE_COMPOSITING_MODE=1 ./tyutool-gui_linux_x86_64_appimage_x.x.x.AppImage` |

### CLI 命令行版

| 平台 | 文件 |
|------|------|
| Linux x86\_64 | `tyutool-cli_linux_x86_64_<版本>.tar.gz` |
| Linux aarch64 | `tyutool-cli_linux_aarch64_<版本>.tar.gz` |
| macOS x86\_64 | `tyutool-cli_macos_x86_64_<版本>.tar.gz` |
| macOS aarch64 | `tyutool-cli_macos_aarch64_<版本>.tar.gz` |
| Windows x86\_64 | `tyutool-cli_windows_x86_64_<版本>.zip` |

解压后直接运行 `tyutool_cli`（Windows 为 `tyutool_cli.exe`）。

## CLI 使用说明

```
tyutool <命令>
```

### 烧录固件

```bash
# 自动检测串口，默认波特率 921600
tyutool write -d bk7231n -f firmware.bin

# 指定串口
tyutool write -d bk7231n -p /dev/ttyUSB0 -f firmware.bin

# 完整参数
tyutool write -d <设备> -p <串口> -b <波特率> -s <起始地址> --end <结束地址> -f <文件>
```

`-d` 支持的值：`bk7231n`、`t2`、`t5`

### 读取 Flash

```bash
# 从地址 0x0 读取 2 MB（默认）
tyutool read -d bk7231n -p /dev/ttyUSB0 -f dump.bin

# 自定义范围
tyutool read -d t5 -p /dev/ttyUSB0 -s 0x0 -l 0x100000 -f dump.bin
```

### 列出串口

```bash
tyutool list-ports
```

### 设备授权

```bash
# 读取当前授权信息
tyutool authorize -p /dev/ttyUSB0

# 写入 UUID 和 AuthKey
tyutool authorize -p /dev/ttyUSB0 --uuid <UUID> --authkey <AUTHKEY>
```

### 复位设备

```bash
tyutool reset -p /dev/ttyUSB0 -d bk7231n
```

### 自升级

```bash
# 检查最新版本
tyutool update --check

# 从 GitHub 升级（默认）
tyutool update

# 从 Gitee 镜像升级
tyutool update --source gitee
```

### 详细日志

```bash
RUST_LOG=debug tyutool write -d bk7231n -f firmware.bin
```

## 从源码构建

**前置依赖：** Rust（stable）、Node.js 22+、pnpm 10+

### 仅构建 CLI

```bash
cargo build -p tyutool-cli --release
# 产物：target/release/tyutool_cli
```

### 构建桌面 GUI

```bash
pnpm install
pnpm run tauri:build
```

### 开发模式

```bash
pnpm install
pnpm run tauri:dev   # GUI 开发服务器（支持热重载）
pnpm run dev:web     # 仅前端开发服务器（不启动 Tauri）
```

## 项目结构

```
tyutool/
├── crates/
│   ├── tyutool-core/   # Rust 库 — 所有烧录逻辑、芯片插件、串口工具
│   └── tyutool-cli/    # 独立 CLI 二进制（仅依赖 tyutool-core）
├── src-tauri/          # Tauri 2 Shell（桌面 GUI 的 Rust 后端）
└── src/                # Vue 3 前端（Vite、Pinia、Tailwind CSS、DaisyUI）
```

`tyutool-core` 被 GUI 和 CLI 共享，所有烧录逻辑集中于此，不在其他地方重复。

## 许可证

Apache-2.0 — 详见 [LICENSE.txt](LICENSE.txt)。
