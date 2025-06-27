import sys
import serial
import serial.tools.list_ports
from PySide6.QtWidgets import QApplication, QWidget, QVBoxLayout, QHBoxLayout, QComboBox, QPushButton, QTextBrowser, QPlainTextEdit, QLabel, QCheckBox
from PySide6.QtCore import QThread, Signal
import re
import datetime


class SerialReadThread(QThread):
    received_signal = Signal(bytes)

    def __init__(self, serial_port):
        super().__init__()
        self.serial_port = serial_port
        self._stop_flag = False

    def run(self):
        while not self._stop_flag:
            try:
                if self.serial_port.is_open and self.serial_port.in_waiting:
                    data = self.serial_port.readline()
                    self.received_signal.emit(data)
            except Exception as e:
                print(f"Error reading from serial port: {e}")

    def stop(self):
        self._stop_flag = True


class SerialAssistant(QWidget):
    def __init__(self):
        super().__init__()
        self.serial_port = None
        self.read_thread = None
        self.show_time = False
        self.show_hex = False
        self.initUI()

    def initUI(self):
        # 布局
        main_layout = QVBoxLayout()

        # 串口选择部分
        port_layout = QHBoxLayout()
        self.port_combobox = QComboBox()
        self.refresh_ports()
        port_layout.addWidget(QLabel("选择串口:"))
        port_layout.addWidget(self.port_combobox)
        refresh_button = QPushButton("刷新串口")
        refresh_button.clicked.connect(self.refresh_ports)
        port_layout.addWidget(refresh_button)
        self.open_close_button = QPushButton("打开串口")
        self.open_close_button.clicked.connect(self.toggle_serial_port)
        port_layout.addWidget(self.open_close_button)
        main_layout.addLayout(port_layout)

        # 功能选择部分
        function_layout = QHBoxLayout()
        self.time_checkbox = QCheckBox("显示接收时间")
        self.time_checkbox.stateChanged.connect(self.toggle_show_time)
        function_layout.addWidget(self.time_checkbox)
        self.hex_checkbox = QCheckBox("以 hex 形式显示")
        self.hex_checkbox.stateChanged.connect(self.toggle_show_hex)
        function_layout.addWidget(self.hex_checkbox)
        main_layout.addLayout(function_layout)

        # 数据显示部分
        self.receive_text = QTextBrowser()
        self.receive_text.setReadOnly(True)
        # 设置接收数据的 QTextBrowser 背景色为黑色
        self.receive_text.setStyleSheet("background-color: black; color: white;")
        main_layout.addWidget(QLabel("接收数据:"))
        main_layout.addWidget(self.receive_text)

        # 数据发送部分
        send_layout = QHBoxLayout()
        self.send_text = QPlainTextEdit()
        send_layout.addWidget(self.send_text)
        send_button = QPushButton("发送数据")
        send_button.clicked.connect(self.send_data)
        send_layout.addWidget(send_button)
        main_layout.addWidget(QLabel("发送数据:"))
        main_layout.addLayout(send_layout)

        self.setLayout(main_layout)
        self.setWindowTitle('串口助手')
        self.setGeometry(300, 300, 800, 600)

    def refresh_ports(self):
        self.port_combobox.clear()
        ports = serial.tools.list_ports.comports()
        for port in ports:
            self.port_combobox.addItem(port.device)

    def toggle_serial_port(self):
        if self.serial_port and self.serial_port.is_open:
            self.close_serial_port()
        else:
            self.open_serial_port()

    def open_serial_port(self):
        try:
            port_name = self.port_combobox.currentText()
            # 修改波特率为 115200
            self.serial_port = serial.Serial(port_name, baudrate=115200, timeout=1)
            self.open_close_button.setText("关闭串口")
            self.read_thread = SerialReadThread(self.serial_port)
            self.read_thread.received_signal.connect(self.update_receive_text)
            self.read_thread.start()
        except serial.SerialException as e:
            print(f"无法打开串口: {e}")

    def close_serial_port(self):
        if self.read_thread:
            self.read_thread.stop()
            self.read_thread.wait()
        if self.serial_port and self.serial_port.is_open:
            self.serial_port.close()
            self.open_close_button.setText("打开串口")

    def send_data(self):
        if self.serial_port and self.serial_port.is_open:
            data = self.send_text.toPlainText()
            if self.show_hex:
                try:
                    data = bytes.fromhex(data.replace(" ", ""))
                except ValueError:
                    print("输入的 hex 数据格式不正确")
                    return
            try:
                if isinstance(data, str):
                    data = data.encode('utf-8')
                self.serial_port.write(data)
                if self.show_hex:
                    send_hex = ' '.join([f'{byte:02x}' for byte in data])
                    self.receive_text.append(f"发送 Hex 数据: {send_hex}")
                else:
                    self.receive_text.append(f"发送数据: {data.decode('utf-8', errors='replace')}")
            except serial.SerialException as e:
                print(f"发送数据时出错: {e}")

    def toggle_show_time(self, state):
        self.show_time = state == 2

    def toggle_show_hex(self, state):
        self.show_hex = state == 2

    def update_receive_text(self, data):
        if self.show_time:
            current_time = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
            prefix = f"[{current_time}] "
        else:
            prefix = ""

        if self.show_hex:
            hex_data = ' '.join([f'{byte:02x}' for byte in data])
            self.receive_text.append(prefix + f"接收 Hex 数据: {hex_data}")
        else:
            try:
                decoded_data = data.decode('utf-8', errors='replace').strip()
            except AttributeError:
                decoded_data = data
            # 解析 ANSI 转义序列并转换为 HTML
            ansi_escape = re.compile(r'\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])')
            html_data = ""
            segments = re.split(r'(\033\[[^m]*m)', decoded_data)
            for segment in segments:
                if segment.startswith('\033[') and segment.endswith('m'):
                    codes = segment[2:-1].split(';')
                    style = ""
                    for code in codes:
                        if code == '0':
                            style = ""
                        elif code == '1':
                            style += "font-weight: bold;"
                        elif code.startswith('3'):
                            color_map = {
                                '30': 'black',
                                '31': 'red',
                                '32': 'green',
                                '33': 'yellow',
                                '34': 'blue',
                                '35': 'magenta',
                                '36': 'cyan',
                                '37': 'white'
                            }
                            style += f"color: {color_map.get(code, 'inherit')};"
                        elif code.startswith('4'):
                            bg_color_map = {
                                '40': 'black',
                                '41': 'red',
                                '42': 'green',
                                '43': 'yellow',
                                '44': 'blue',
                                '45': 'magenta',
                                '46': 'cyan',
                                '47': 'white'
                            }
                            style += f"background-color: {bg_color_map.get(code, 'inherit')};"
                    if style:
                        html_data += f'<span style="{style}">'
                    else:
                        html_data += '</span>'
                else:
                    html_data += segment
            # 去除未处理的 ANSI 转义序列
            html_data = ansi_escape.sub('', html_data)
            self.receive_text.append(prefix + html_data)

    def closeEvent(self, event):
        if self.read_thread:
            self.read_thread.stop()
            self.read_thread.wait()
        if self.serial_port and self.serial_port.is_open:
            self.serial_port.close()
        event.accept()

if __name__ == '__main__':
    app = QApplication(sys.argv)
    assistant = SerialAssistant()
    assistant.show()
    sys.exit(app.exec())
