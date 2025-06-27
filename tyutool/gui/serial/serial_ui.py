#!/usr/bin/env python
# -*- coding: utf-8 -*-

import os
import sys
from datetime import datetime
import re
import binascii
import serial
from serial.tools import list_ports

from PySide6 import QtWidgets
from PySide6.QtWidgets import QFileDialog, QDialog
from PySide6.QtCore import QThread, Signal
from PySide6.QtWidgets import (
    QLineEdit, QHBoxLayout, QVBoxLayout, QDialogButtonBox, QPushButton)

from tyutool.util.util import tyutool_root


class SerialReadThread(QThread):
    received_signal = Signal(bytes)

    def __init__(self, serial_port, logger):
        super().__init__()
        self.serial_port = serial_port
        self.logger = logger
        self._stop_flag = False

    def run(self):
        while not self._stop_flag:
            try:
                if self.serial_port.is_open and self.serial_port.in_waiting:
                    data = self.serial_port.readline()
                    self.received_signal.emit(data)
            except Exception as e:
                self.logger.error(f"Error reading from serial port: {e}")

    def stop(self):
        self._stop_flag = True


class SerialGUI(QtWidgets.QMainWindow):
    def __init__(self):
        super().__init__()
        self.serial_port = None
        self.read_thread = None
        pass

    def serialUiSetup(self):
        self.ui.comboBoxSBaud.addItems(["115200", "230400",
                                        "460800", "921600"])
        self.ui.comboBoxDataBits.addItems(["8", "7", "6", "5"])
        self.ui.comboBoxParity.addItems(["None", "Odd", "Even",
                                         "Mark", "Space"])
        self.ui.comboBoxStopBits.addItems(["1", "1.5", "2"])

        self.ui.pushButtonCom.clicked.connect(self.btnComClicked)
        self.ui.pushButtonClear.clicked.connect(self.btnClearClicked)
        self.ui.pushButtonSave.clicked.connect(self.btnSaveClicked)
        self.ui.pushButtonSStart.clicked.connect(self.btnSerStartClicked)
        self.ui.pushButtonSend.clicked.connect(self.btnSendClicked)
        self.ui.pushButtonAuth.clicked.connect(self.btnAuthClicked)
        self.ui.pushButtonSend.setEnabled(False)
        self.ui.pushButtonAuth.setEnabled(False)
        self.ui.textBrowserRx.setStyleSheet("background-color: black; color: white;")
        pass

    def btnComClicked(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        port_list = list(list_ports.comports())
        port_items = []
        self.ui.comboBoxCom.clear()
        if len(port_list) > 0:
            for port in port_list:
                pl = list(port)
                if pl[0].startswith("/dev/ttyS"):
                    continue
                self.logger.debug(f'port info: {pl}')
                port_items.append(pl[0])
        self.ui.comboBoxCom.addItems(port_items)
        pass

    def btnClearClicked(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        self.ui.textBrowserRx.clear()
        pass

    def btnSaveClicked(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        default_path = os.path.join(tyutool_root(), "log.log")
        file_path, _ = QFileDialog.getSaveFileName(
            self,
            "Save",
            default_path,
            "LOG (*.log)"
        )
        if not file_path:
            return False
        self.logger.debug(f'save file: {file_path}')
        context = self.ui.textBrowserRx.toPlainText()
        f = open(file_path, 'w', encoding='utf8')
        f.write(context)
        f.close()
        pass

    def btnSerStartClicked(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        data_bits_map = {
            "5": serial.FIVEBITS,
            "6": serial.SIXBITS,
            "7": serial.SEVENBITS,
            "8": serial.EIGHTBITS,
        }
        parity_map = {
            "None": serial.PARITY_NONE,
            "Odd": serial.PARITY_ODD,
            "Even": serial.PARITY_EVEN,
            "Mark": serial.PARITY_MARK,
            "Space": serial.PARITY_SPACE,
        }
        stop_bits_map = {
            "1": serial.STOPBITS_ONE,
            "1.5": serial.STOPBITS_ONE_POINT_FIVE,
            "2": serial.STOPBITS_TWO,
        }
        com_name = self.ui.comboBoxCom.currentText()
        baudrate = int(self.ui.comboBoxSBaud.currentText())
        data_bits = self.ui.comboBoxDataBits.currentText()
        parity = self.ui.comboBoxParity.currentText()
        stop_bits = self.ui.comboBoxStopBits.currentText()
        try:
            self.serial_port = serial.Serial(com_name,
                                             baudrate=baudrate,
                                             bytesize=data_bits_map[data_bits],
                                             stopbits=stop_bits_map[stop_bits],
                                             parity=parity_map[parity],
                                             timeout=1)
            self.read_thread = SerialReadThread(self.serial_port,
                                                self.logger)
            self.read_thread.received_signal.connect(self.update_receive_text)
            self.read_thread.start()
        except Exception as e:
            self.logger.error(e)
            self.logger.error(f'Open COM error: {com_name}!')
            return False

        self.ui.pushButtonCom.setEnabled(False)
        self.ui.comboBoxCom.setEnabled(False)
        self.ui.comboBoxSBaud.setEnabled(False)
        self.ui.comboBoxDataBits.setEnabled(False)
        self.ui.comboBoxParity.setEnabled(False)
        self.ui.comboBoxStopBits.setEnabled(False)
        self.ui.pushButtonSend.setEnabled(True)
        self.ui.pushButtonAuth.setEnabled(True)
        self.ui.pushButtonSStart.setText("Stop")
        self.ui.pushButtonSStart.clicked.disconnect()
        self.ui.pushButtonSStart.clicked.connect(self.btnSerStopClicked)
        pass

    def btnSerStopClicked(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        if self.read_thread:
            self.read_thread.stop()
            self.read_thread.wait()
        if self.serial_port and self.serial_port.is_open:
            self.serial_port.close()
        self.ui.pushButtonCom.setEnabled(True)
        self.ui.comboBoxCom.setEnabled(True)
        self.ui.comboBoxSBaud.setEnabled(True)
        self.ui.comboBoxDataBits.setEnabled(True)
        self.ui.comboBoxParity.setEnabled(True)
        self.ui.comboBoxStopBits.setEnabled(True)
        self.ui.pushButtonSend.setEnabled(False)
        self.ui.pushButtonAuth.setEnabled(False)
        self.ui.pushButtonSStart.setText("Start")
        self.ui.pushButtonSStart.clicked.disconnect()
        self.ui.pushButtonSStart.clicked.connect(self.btnSerStartClicked)
        pass

    def _send_data(self, txbuf):
        try:
            self.logger.debug(f'Send: {txbuf}')
            self.serial_port.write(txbuf)
        except Exception as e:
            self.logger.error(e)
            self.logger.error("Failed to write data to the serial port")
            return False

        if self.ui.checkBoxTxTime.isChecked():
            txbuf = self.getTimeStamp() + f"{txbuf}"

        show_data = f'<font color=\"#008600\">{txbuf}</font>'
        self.ui.textBrowserRx.append(show_data+"\n")
        pass

    def btnSendClicked(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        send_data = self.ui.plainTextEditTx.toPlainText()
        if len(send_data) == 0:
            return True
        if self.ui.checkBoxTxHex.isChecked():
            send_data = send_data.replace(' ', '')  # 去空格
            # 偶数对齐
            send_data = send_data[0:len(send_data) - len(send_data) % 2]
            if not send_data.isalnum():
                self.logger.error("The sent data contains \
non-hexadecimal data")
                return False
            try:
                txbuf = binascii.a2b_hex(send_data)
            except Exception as e:
                self.logger.error(e)
                self.logger.error("Failed to convert hexadecimal data")
                return False
        else:
            txbuf = send_data.encode('utf-8')

        self._send_data(txbuf)
        pass

    def getTimeStamp(self):
        stamp = datetime.now().strftime('%H:%M:%S.%f')[:-3]
        stamp = f'[{stamp}] '
        return stamp

    def update_receive_text(self, data):
        prefix = ""
        if self.ui.checkBoxRxTime.isChecked():
            prefix = self.getTimeStamp()

        if self.ui.checkBoxRxHex.isChecked():
            hexbuf = binascii.b2a_hex(data).decode('ascii')
            self.ui.textBrowserRx.append(prefix + f"{hexbuf}")
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
                    html_data += segment
            # 去除未处理的 ANSI 转义序列
            html_data = ansi_escape.sub('', html_data)
            self.ui.textBrowserRx.append(prefix + html_data)

    def auth_function(self, uuid, authkey):
        send_data = f"auth {uuid} {authkey}\n"
        txbuf = send_data.encode('utf-8')
        self._send_data(txbuf)
        pass

    def btnAuthClicked(self):
        dialog = AuthDialog(self.auth_function)
        dialog.exec()
        pass

    def closeEvent(self, event):
        if self.read_thread:
            self.read_thread.stop()
            self.read_thread.wait()
        if self.serial_port and self.serial_port.is_open:
            self.serial_port.close()
        event.accept()


class AuthDialog(QDialog):
    def __init__(self, auth_func):
        super().__init__()
        self.auth_func = auth_func
        self.setWindowTitle("Auth")
        self.setFixedSize(300, 200)
        layout = QVBoxLayout()

        self.uuid_input = QLineEdit()
        self.uuid_input.setPlaceholderText("UUID")
        name_layout = QHBoxLayout()
        name_layout.addWidget(self.uuid_input)
        layout.addLayout(name_layout)

        self.authkey_input = QLineEdit()
        self.authkey_input.setPlaceholderText("AUTHKEY")
        password_layout = QHBoxLayout()
        password_layout.addWidget(self.authkey_input)
        layout.addLayout(password_layout)

        self.button_box = QDialogButtonBox(QDialogButtonBox.NoButton)
        show_button = QPushButton("AUTHORIZE")
        show_button.clicked.connect(self.on_ok_clicked)
        self.button_box.addButton(show_button, QDialogButtonBox.AcceptRole)
        layout.addWidget(self.button_box)

        self.setLayout(layout)

    def on_ok_clicked(self):
        uuid = self.uuid_input.text()
        authkey = self.authkey_input.text()
        self.auth_func(uuid, authkey)
