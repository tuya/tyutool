[简体中文](README_zh.md)

# tyuTool - A Universal Serial Port Tool for Tuya

`tyuTool` is a cross-platform serial port utility designed for Internet of Things (IoT) developers to flash and read firmware for various mainstream chips. It provides both a simple Graphical User Interface (GUI) and a powerful Command-Line Interface (CLI) to streamline development and debugging workflows.

---

## ✨ Features

- **Dual-Mode Operation**: Offers both an intuitive **GUI** and a flexible **CLI** to meet the needs of different scenarios.
- **Core Serial Functions**: Supports **firmware flashing** (writing to Flash) and **firmware reading** (reading from Flash).
- **Cross-Platform Support**: Fully compatible with **Windows, Linux, and macOS** (x86 & ARM64).
- **Multi-Chip Support**: Built-in flashing protocols for a variety of chips, easily handling different projects.
- **User-Friendly**: Clean user interface, with the CLI providing detailed progress bars and status feedback.
- **Standalone Executables**: Provides portable executables that run without needing a Python environment.

## Supported Chips

This tool currently supports (but is not limited to) the following chip platforms:

- BK7231N / BK7231T
- RTL8720CF
- ESP32 / ESP32-C3 / ESP32-S3
- LN882H
- T5
- ...

## 🚀 Quick Start

We offer two ways to use the tool. Please choose the one that suits your needs.

### Method 1: Run from Source (Recommended for Developers)

Follow these steps if you want to do secondary development or run directly from the source code.

1.  **Prerequisites**:
    - Ensure you have Python 3.8+ installed.
    - Install Git.

2.  **Clone the Repository**:
    ```bash
    git clone https://github.com/your-repo/tyutool.git
    cd tyutool
    ```

3.  **Create a Virtual Environment and Install Dependencies**:
    We provide automated scripts for this step.
    - **Linux / macOS**:
      ```bash
      . ./export.sh
      ```
    - **Windows**:
      ```bash
      .\export.bat
      ```
    This script will automatically create a `venv` virtual environment and install all dependencies from `requirements.txt` using `pip`.

4.  **(Important) Linux Serial Port Permissions**:
    On Linux systems, the default user may not have permission to access serial ports. Please execute the following command to add the current user to the `dialout` group:
    ```bash
    sudo usermod -aG dialout $USER
    ```
    **You must restart or log out and log back in for the permissions to take effect!**

### Method 2: Use Pre-compiled Executables

This is the simplest and fastest way, requiring no installation of Python or any dependencies.

1.  **Download**: Choose the latest version for your operating system from the links below.

    > - [github](https://github.com/tuya/tyutool/releases)

    > - [gitee](https://gitee.com/tuya-open/tyutool/releases)

2.  **Extract**: Unzip the downloaded archive to any directory.
3.  **Run**:
    - **Linux/macOS**: Run the extracted file directly from your terminal, e.g., `./tyutool_gui`.
    - **Windows**: Double-click `tyutool_gui.exe` to run.
4.  **(Important) Install Drivers**: Ensure you have the correct USB to UART driver (e.g., CP210x, CH340) for your target chip installed on your computer, otherwise the tool will not be able to find the serial port.

## 📖 Usage Guide

### Graphical User Interface (GUI)

Start the GUI by running `tyutool_gui.py` or the corresponding executable.

```bash
# Start from source
python tyutool_gui.py
```

**Steps**:
1.  **Select Chip (Device)**: Choose your target chip model from the dropdown menu.
2.  **Select Port**: Click the refresh button, then select the serial port your device is connected to from the dropdown menu.
3.  **Set Baud Rate**: Enter the flashing baud rate according to your hardware requirements (default is `115200`).
4.  **Select File**: Click the "..." button next to "File" to select the `.bin` firmware file to be flashed.
5.  **Execute Action**: Click the "Write" or "Read" button to start the task. The progress bar will show the current status.

### Command-Line Interface (CLI)

Run `tyutool_cli.py` or the corresponding executable and control its behavior with arguments.

```bash
# Start from source
python tyutool_cli.py --help
```

**Common Command Examples**:

- **Flash Firmware (Write)**
  Flash the `bk.bin` file to a `BK7231N` chip on port `/dev/ttyACM0` with a baud rate of `2000000`.
  ```bash
  # Syntax: tyutool_cli.py write -d <chip> -p <port> -b <baudrate> -f <filepath>
  python tyutool_cli.py write -d BK7231N -p /dev/ttyACM0 -b 2000000 -f ./bk.bin
  ```

- **Read Firmware (Read)**
  Read `0x200000` bytes of data starting from address `0x11000` from a `BK7231N` chip and save it as `read.bin`.
  ```bash
  # Syntax: tyutool_cli.py read -d <chip> -p <port> -b <baudrate> -s <start_address> -l <length> -f <save_path>
  python tyutool_cli.py read -d BK7231N -p /dev/ttyACM0 -b 2000000 -s 0x11000 -l 0x200000 -f read.bin
  ```

- **AI Debug (Debug)**
  Provides AI debug tools that support collecting and analyzing various data types such as audio, video, images, and text through network or serial connections.

  1. **Web Mode (web)**: Connect to a debug server over the network to monitor specified data types in real-time.
     ```bash
     # Syntax: tyutool_cli.py debug web -i <server_ip> -p <port> -e <event_type> -s <save_dir>
     # Monitor text data, connecting to localhost on port 5055
     python tyutool_cli.py debug web -i localhost -p 5055 -e t -s web_ai_debug

     # Monitor both audio and video data
     python tyutool_cli.py debug web -i 192.168.1.100 -p 5055 -e a -e v -s debug_output
     ```
     Event type parameters (-e):
     - `a`: Audio data
     - `t`: Text data
     - `v`: Video data
     - `p`: Image data

  2. **Serial Mode (ser)**: Connect to devices via serial port for AI debugging with interactive commands.
     ```bash
     # Syntax: tyutool_cli.py debug ser -p <port> -b <baudrate> -s <save_dir>
     python tyutool_cli.py debug ser -p /dev/ttyUSB0 -b 460800 -s ser_ai_debug
     ```
     After entering interactive mode, you can use built-in commands to control the debugging process.

  3. **Serial Auto Mode (ser_auto)**: Automated serial debugging test without manual interaction.
     ```bash
     # Syntax: tyutool_cli.py debug ser_auto -p <port> -b <baudrate> -s <save_dir>
     python tyutool_cli.py debug ser_auto -p /dev/ttyUSB0 -b 460800 -s auto_debug
     ```

## 📝 Development Notes

For more details on the project's build process, packaging, and code structure, please refer to the [Development Documentation](tools/develop.md).

## 📁 Other Documents

- [T5-Audio-Debug-Guide](docs/en/T5-Audio-Debug-Guide.md)

## 📄 License

This project is licensed under the [Apache-2.0](LICENSE) License.

## Disclaimer / 免责声明

The English and Chinese texts below state the same terms. If there is any conflict or discrepancy between the two language versions, **the English version shall prevail**.

以下英文与中文表述同一套条款。两种文本如有冲突或不一致，**以英文版为准**。

---

### Legal disclaimer & no affiliation / 法律声明及无关联声明

**English**

TuyaOpen is an independent, community-driven open-source project. This project and its production flashing tool (the “Software”) are **NOT** affiliated with, endorsed by, sponsored by, or connected in any way to Tuya Inc. or any of its subsidiaries or affiliates.

By downloading, installing, executing, or otherwise using the Software, you (“User”, “Developer”, “Manufacturer”, or “Customer”) acknowledge that you have read, fully understood, and unconditionally agreed to the following terms.

**简体中文**

TuyaOpen 为独立、社区驱动的开源项目。本项目及其量产烧录相关软件（下称「本软件」）与涂鸦智能（Tuya Inc.）及其任何子公司或关联公司**均无关联**，亦**未获其背书、赞助或以任何形式与之存在联系**。

凡下载、安装、运行或以其他方式使用本软件，即表示您（「用户」「开发者」「制造商」或「客户」）已阅读、充分理解并无条件同意下列全部条款。

---

### User testing responsibility / 用户测试责任

**English**

You acknowledge and agree that you are solely responsible for rigorously testing the Software, its functionality, compatibility, stability, safety, and performance with your hardware, firmware, production processes, and end products before any mass production, deployment, or commercial use. No assurances are provided regarding suitability for any specific manufacturing environment without your own comprehensive validation.

**简体中文**

您确认并同意：在将本软件用于任何量产、部署或商业用途之前，**由您独自负责**对本软件的功能、兼容性、稳定性、安全性及性能，结合您的硬件、固件、生产工艺与最终产品进行**充分、严格的测试与验证**。在未由您自行完成全面验证的情况下，**不就任何特定制造环境的适用性作出任何保证**。

---

### Disclaimer of warranty / 免责声明（无担保）

**English**

To the fullest extent permitted by applicable law: The Software is provided “AS IS” and “WITH ALL FAULTS”, without any warranty, condition, or representation of any kind, whether express, implied, or statutory. All implied warranties, including but not limited to implied warranties of merchantability, fitness for a particular purpose, accuracy, reliability, completeness, non-infringement, title, or quiet enjoyment, are hereby expressly disclaimed. No oral or written information, guidance, or support provided by the project maintainers or contributors shall create any warranty. We do not warrant that the Software will operate error-free, uninterrupted, compatible with all devices, or meet any production requirements.

**简体中文**

在适用法律允许的最大范围内：本软件按「**现状**」及「**包含一切已知与未知缺陷**」提供，**不提供任何形式的明示、默示或法定担保、条件或陈述**。**所有默示担保均被明确排除**，包括但不限于：适销性、特定用途适用性、准确性、可靠性、完整性、不侵权、权属或安静享用等。项目维护者或贡献者提供的任何口头或书面信息、指导或支持**均不构成任何担保**。我们**不保证**本软件无错误、不间断运行、与所有设备兼容或满足任何量产要求。

---

### Limitation of liability / 责任限制

**English**

Under no circumstances shall the TuyaOpen project maintainers, contributors, copyright holders, or affiliated parties be liable for any direct, indirect, incidental, special, punitive, exemplary, or consequential damages, including but not limited to: production downtime, lost profits, lost revenue, or business interruption; device bricking, hardware damage, firmware corruption, or manufacturing failures; data loss, production line disruption, or failure in mass deployment; claims by third parties arising from the use or misuse of the Software; or any other commercial or physical harm resulting from deployment in production environments. This limitation applies regardless of legal theory, including contract, tort (including negligence), strict liability, or otherwise, even if advised of the possibility of such damages. Your sole and exclusive remedy for dissatisfaction with the Software is to immediately discontinue all use.

**简体中文**

在任何情况下，**TuyaOpen 项目维护者、贡献者、版权持有人或相关方**均不对下列损害承担责任，无论基于合同、侵权（含过失）、严格责任或其他法理，**即使已被告知可能发生此类损害**：停产、利润或收入损失、业务中断；设备变砖、硬件损坏、固件损坏或制造失败；数据丢失、产线中断或大规模部署失败；因使用或**误用**本软件而引发的第三方索赔；在生产环境中部署所导致的任何其他商业或物理损害。**您对因不满本软件而产生的唯一且排他的救济为：立即停止使用本软件。**

---

### Assumption of risk / 风险自担

**English**

You assume full and exclusive responsibility and risk for: selection and use of the Software for production purposes; all flashing, programming, and deployment operations; compliance with applicable laws, industry standards, and hardware specifications; and all costs related to repair, rework, or production losses.

**简体中文**

您**自行承担全部且排他的责任与风险**，包括但不限于：为生产目的选择并使用本软件；一切烧录、编程与部署操作；遵守适用法律、行业标准与硬件规格；与维修、返工或生产损失相关的全部费用。

---

### Governing provisions / 其他条款

**English**

If any provision of this disclaimer is found to be unenforceable, the remaining provisions shall remain in full force and effect.

**简体中文**

若本免责声明中任一条款被认定为不可执行，其余条款仍应在法律允许范围内**完全有效**。

---

## FAQ (Frequently Asked Questions)

### 1. Driver Downloads

If your computer cannot recognize the serial port, it is usually because the USB to UART driver is missing.

Here are the download links for common **CH34x** series chip drivers:

- **Windows**: [CH343SER_EXE](https://www.wch.cn/downloads/ch343ser_exe.html)

- **macOS**: [CH34XSER_MAC_ZIP](https://www.wch.cn/downloads/CH34XSER_MAC_ZIP.html)

Here are the download links for common **CP210x** series chip drivers:

- **Windows**: [CP210x_Windows_Drivers](https://www.silabs.com/documents/public/software/CP210x_Windows_Drivers.zip)
- **macOS**: [CP210x_Mac_Drivers](https://www.silabs.com/documents/public/software/Mac_OSX_VCP_Driver.zip)
