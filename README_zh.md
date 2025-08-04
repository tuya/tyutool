[English](README.md)

# tyuTool - 涂鸦通用串口工具

`tyuTool` 是一款为物联网（IoT）开发者设计的、跨平台的串口工具，用于烧录和读取多种主流芯片的固件。它提供简洁的图形用户界面（GUI）和强大的命令行界面（CLI），旨在简化开发和调试流程。

---

## ✨ 功能特性

- **双模式操作**: 提供直观的 **图形界面 (GUI)** 和灵活的 **命令行 (CLI)**，满足不同场景下的使用需求。
- **核心串口功能**: 支持 **固件烧录**（写入 Flash）和 **固件读取**（从 Flash 读出）。
- **跨平台支持**: 完美兼容 **Windows, Linux, 和 macOS** (x86 & ARM64)。
- **多芯片支持**: 内置多种芯片的烧录协议，轻松应对不同项目。
- **用户友好**: 操作界面简洁，CLI 提供详细的进度条和状态反馈。
- **独立打包**: 提供免安装的绿色可执行文件，无需配置 Python 环境即可使用。

## 支持芯片

本工具目前主要支持（但不限于）以下芯片平台：

- BK7231N / BK7231T
- RTL8720CF
- ESP32 / ESP32-C3 / ESP32-S3
- LN882H
- T5
- ...

## 🚀 快速开始 (Quick Start)

我们提供两种使用方式，请根据您的需求选择。

### 方式一：从源码运行 (推荐给开发者)

如果您希望进行二次开发或从源码直接运行，请按以下步骤操作。

1.  **环境准备**:
    - 确保您已安装 Python 3.8+。
    - 安装 Git。

2.  **克隆仓库**:
    ```bash
    git clone https://github.com/your-repo/tyutool.git
    cd tyutool
    ```

3.  **创建虚拟环境并安装依赖**:
    我们提供了自动化脚本来完成此步骤。
    - **Linux / macOS**:
      ```bash
      . ./export.sh
      ```
    - **Windows**:
      ```bash
      .\export.bat
      ```
    此脚本会自动创建 `venv` 虚拟环境并使用 `pip` 安装 `requirements.txt` 中的所有依赖。

4.  **(重要) Linux 串口权限**:
    在 Linux 系统下，默认用户可能没有串口访问权限。请执行以下命令将当前用户添加到 `dialout` 组：
    ```bash
    sudo usermod -aG dialout $USER
    ```
    **执行后必须重启或注销并重新登录，权限才会生效！**

### 方式二：使用预编译的可执行程序

这是最简单快捷的方式，无需安装 Python 或任何依赖。

1.  **下载**: 从下面的链接中选择对应您操作系统的最新版本下载。

    > - [github](https://github.com/tuya/tyutool/releases)

    > - [gitee](https://gitee.com/tuya-open/tyutool/releases)

2.  **解压**: 将下载的压缩包解压到任意目录。
3.  **运行**:
    - **Linux/macOS**: 在终端中直接运行解压后的文件，例如 `./tyutool_gui`。
    - **Windows**: 双击 `tyutool_gui.exe` 运行。
4.  **(重要) 安装驱动**: 请确保您的电脑已正确安装目标芯片的 USB to UART 驱动（如 CP210x, CH340 等），否则工具将无法找到串口。


## 📖 使用说明 (Usage Guide)

### 图形界面 (GUI)

直接运行 `tyutool_gui.py` 或对应的可执行程序即可启动。

```bash
# 从源码启动
python tyutool_gui.py
```

**操作步骤**:
1.  **选择芯片 (Device)**: 在下拉菜单中选择您的目标芯片型号。
2.  **选择串口 (Port)**: 点击刷新按钮，然后在下拉菜单中选择您的设备所连接的串口。
3.  **设置波特率 (Baud)**: 根据您的硬件要求输入烧录波特率（默认为 `115200`）。
4.  **选择文件**: 点击 "File" 旁边的 "..." 按钮选择要烧录的固件 `.bin` 文件。
5.  **执行操作**: 点击 "Write" (烧录) 或 "Read" (读取) 按钮开始任务。进度条将显示当前进度。

### 命令行 (CLI)

运行 `tyutool_cli.py` 或对应的可执行程序，并通过参数控制其行为。

```bash
# 从源码启动
python tyutool_cli.py --help
```

**常用命令示例**:

- **烧录固件 (Write)**
  将 `bk.bin` 文件烧录到 `BK7231N` 芯片，串口为 `/dev/ttyACM0`，波特率为 `2000000`。
  ```bash
  # 语法: tyutool_cli.py write -d <芯片> -p <串口> -b <波特率> -f <文件路径>
  python tyutool_cli.py write -d BK7231N -p /dev/ttyACM0 -b 2000000 -f ./bk.bin
  ```

- **读取固件 (Read)**
  从 `BK7231N` 芯片的 `0x11000` 地址开始，读取 `0x200000` 长度的数据，并保存为 `read.bin`。
  ```bash
  # 语法: tyutool_cli.py read -d <芯片> -p <串口> -b <波特率> -s <起始地址> -l <长度> -f <保存路径>
  python tyutool_cli.py read -d BK7231N -p /dev/ttyACM0 -b 2000000 -s 0x11000 -l 0x200000 -f read.bin
  ```

- **AI 调试 (Debug)**
  提供 AI 调试工具，支持通过网络或串口采集和分析音频、视频、图像、文本等多种数据类型。

  1. **Web 模式 (web)**: 通过网络连接到调试服务器，实时监控指定数据类型。
     ```bash
     # 语法: tyutool_cli.py debug web -i <服务器IP> -p <端口> -e <事件类型> -s <保存目录>
     # 监控文本数据，连接到本地服务器的 5055 端口
     python tyutool_cli.py debug web -i xxx.xx.x.xxx -p 5055 -e t

     # 同时监控音频和视频数据
     python tyutool_cli.py debug web -i xxx.xx.x.xxx -p 5055 -e a -e v
     ```
     事件类型参数 (-e):
     - `a`: 音频数据 (Audio)
     - `t`: 文本数据 (Text)
     - `v`: 视频数据 (Video)
     - `p`: 图像数据 (Image)

  2. **串口模式 (ser)**: 通过串口连接设备进行 AI 调试，支持交互式命令。
     ```bash
     # 语法: tyutool_cli.py debug ser -p <串口> -b <波特率> -s <保存目录>
     python tyutool_cli.py debug ser -p /dev/ttyUSB0 -b 460800
     ```
     进入交互模式后，可使用内置命令控制调试过程。
  3. **串口自动模式 (ser_auto)**: 自动化串口调试测试，无需人工交互。
     ```bash
     # 语法: tyutool_cli.py debug ser_auto -p <串口> -b <波特率> -s <保存目录>
     python tyutool_cli.py debug ser_auto -p /dev/ttyUSB0 -b 460800 -s auto_debug
     ```

## 📝 开发说明

关于项目的构建、打包和代码结构的更多详细信息，请参考 [开发文档](tools/develop.md)。

## 📄 许可证 (License)

本项目采用 [Apache-2.0](LICENSE) 许可证。

## 常见问题 (FAQ)

### 1. 驱动下载

如果您的电脑无法识别到串口，通常是因为缺少 USB to UART 驱动。

以下是常用 **CH34x** 系列芯片的驱动下载链接：

- **Windows**: [CH343SER_EXE](https://www.wch.cn/downloads/ch343ser_exe.html)

- **macOS**: [CH34XSER_MAC_ZIP](https://www.wch.cn/downloads/CH34XSER_MAC_ZIP.html)

以下是常用 **CP210x** 系列芯片的驱动下载链接：

- **Windows**: [CP210x_Windows_Drivers](https://www.silabs.com/documents/public/software/CP210x_Windows_Drivers.zip)
- **macOS**: [CP210x_Mac_Drivers](https://www.silabs.com/documents/public/software/Mac_OSX_VCP_Driver.zip)
