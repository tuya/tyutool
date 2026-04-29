# tyutool

[![Release](https://img.shields.io/github/v/release/tuya/tyutool?style=flat-square)](https://github.com/tuya/tyutool/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/tuya/tyutool/release.yml?style=flat-square&label=构建)](https://github.com/tuya/tyutool/actions/workflows/release.yml)
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
| Linux x86\_64 | `.deb`、`.rpm`、`.AppImage`、便携版 tar.gz |
| Linux aarch64 | `.deb`、`.AppImage`、便携版 tar.gz |
| macOS（Universal） | `.dmg`、便携版 tar.gz |
| Windows x86\_64 | NSIS 安装包（`.exe`）、便携版 `.zip` |

GUI 支持应用内自动更新。

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
