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
- ... (and other chips that can be flashed using protocols like `esptool.py`)

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

    > - [Linux-CLI](https://images.tuyacn.com/smart/embed/package/vscode/data/ide_serial/tyutool_cli.tar.gz)
    > - [Linux-GUI](https://images.tuyacn.com/smart/embed/package/vscode/data/ide_serial/tyutool_gui.tar.gz)
    > - [Windows-CLI](https://images.tuyacn.com/smart/embed/package/vscode/data/ide_serial/win_tyutool_cli.tar.gz)
    > - [Windows-GUI](https://images.tuyacn.com/smart/embed/package/vscode/data/ide_serial/win_tyutool_gui.tar.gz)
    > - [MAC-ARM64-CLI](https://images.tuyacn.com/smart/embed/package/vscode/data/ide_serial/darwin_arm64_tyutool_cli.tar.gz)
    > - [MAC-ARM64-GUI](https://images.tuyacn.com/smart/embed/package/vscode/data/ide_serial/darwin_arm64_tyutool_gui.tar.gz)
    > - [MAC-X86-CLI](https://images.tuyacn.com/smart/embed/package/vscode/data/ide_serial/darwin_x86_tyutool_cli.tar.gz)
    > - [MAC-X86-GUI](https://images.tuyacn.com/smart/embed/package/vscode/data/ide_serial/darwin_x86_tyutool_gui.tar.gz)

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

## 📝 Development Notes

For more details on the project's build process, packaging, and code structure, please refer to the [Development Documentation](tools/develop.md).

## 📄 License

This project is licensed under the [Apache-2.0](LICENSE) License.

## FAQ (Frequently Asked Questions)

### 1. Driver Downloads

If your computer cannot recognize the serial port, it is usually because the USB to UART driver is missing.

Here are the download links for common **CH34x** series chip drivers:

- **Windows**: [CH343SER_EXE](https://www.wch.cn/downloads/ch343ser_exe.html)
- **macOS**: [CH34XSER_MAC_ZIP](https://www.wch.cn/downloads/CH34XSER_MAC_ZIP.html)

Here are the download links for common **CP210x** series chip drivers:

- **Windows**: [CP210x_Windows_Drivers](https://www.silabs.com/documents/public/software/CP210x_Windows_Drivers.zip)
- **macOS**: [CP210x_Mac_Drivers](https://www.silabs.com/documents/public/software/Mac_OSX_VCP_Driver.zip)
