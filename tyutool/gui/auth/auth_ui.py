#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
from serial.tools import list_ports

from PySide6 import QtWidgets
from PySide6.QtCore import QThread, Signal
from PySide6.QtWidgets import QFileDialog, QMessageBox

from tyutool.auth import AuthHandler, AuthExcelParser
from tyutool.auth.auth_handler import mask_authkey

SUPPORTED_CHIPS = ["ESP32", "ESP32-C3", "ESP32-C6", "ESP32-S3"]


class AuthWorker(QThread):
    """Background thread that runs single-device authorization."""
    log_signal = Signal(str, str)
    progress_signal = Signal(int, int)
    device_info_signal = Signal(str, str, str)
    stats_signal = Signal(int, int, int)
    finished_signal = Signal(bool)

    def __init__(self, handler, port, baudrate, excel_path):
        super().__init__()
        self.handler = handler
        self.port = port
        self.baudrate = baudrate
        self.excel_path = excel_path

    def run(self):
        self.handler.on_log = lambda level, line: self.log_signal.emit(level, line)
        self.handler.on_progress = lambda cur, tot: self.progress_signal.emit(cur, tot)
        self.handler.on_device_info = lambda mac, uuid, st: self.device_info_signal.emit(mac, uuid, st)
        self.handler.on_stats = lambda t, u, r: self.stats_signal.emit(t, u, r)
        result = self.handler.authorize_single(
            self.port, self.baudrate, self.excel_path)
        self.finished_signal.emit(result)


class ReadMACWorker(QThread):
    """Background thread for reading device MAC."""
    mac_signal = Signal(str)
    log_signal = Signal(str, str)

    def __init__(self, port, baudrate):
        super().__init__()
        self.port = port
        self.baudrate = baudrate

    def run(self):
        handler = AuthHandler()
        handler.on_log = lambda level, line: self.log_signal.emit(level, line)
        mac = handler.read_mac_only(self.port, self.baudrate)
        self.mac_signal.emit(mac or "")
        handler.cleanup()


class AuthGUI(QtWidgets.QMainWindow):
    def __init__(self):
        super().__init__()
        self._auth_handler = None
        self._auth_worker = None
        self._mac_worker = None
        self._auth_excel_path = ""

    def authUiSetup(self):
        self.ui.comboBoxAuthChip.addItems(SUPPORTED_CHIPS)
        self.ui.comboBoxAuthBaud.addItems(["115200", "230400", "460800", "921600"])

        self.ui.pushButtonAuthStart.setEnabled(False)
        self.ui.pushButtonAuthStop.setEnabled(False)
        self.ui.pushButtonAuthReadMAC.setEnabled(False)

        self.ui.pushButtonAuthRescan.clicked.connect(self._authRescanPorts)
        self.ui.pushButtonAuthBrowse.clicked.connect(self._authBrowseExcel)
        self.ui.pushButtonAuthStart.clicked.connect(self._authStart)
        self.ui.pushButtonAuthStop.clicked.connect(self._authStop)
        self.ui.pushButtonAuthReadMAC.clicked.connect(self._authReadMAC)
        self.ui.comboBoxAuthPort.currentTextChanged.connect(self._authUpdateButtons)

        self._authRescanPorts()

    def _authRescanPorts(self):
        self.ui.comboBoxAuthPort.clear()
        port_items = []
        port_list = list(list_ports.comports())
        for port in port_list:
            pl = list(port)
            if pl[0].startswith("/dev/ttyS"):
                continue
            port_items.append(pl[0])
        port_items.sort()
        self.ui.comboBoxAuthPort.addItems(port_items)
        self._authUpdateButtons()

    def _authUpdateButtons(self):
        has_port = bool(self.ui.comboBoxAuthPort.currentText())
        has_excel = bool(self._auth_excel_path)
        self.ui.pushButtonAuthStart.setEnabled(has_port and has_excel)
        self.ui.pushButtonAuthReadMAC.setEnabled(has_port)

    def _authBrowseExcel(self):
        filepath, _ = QFileDialog.getOpenFileName(
            self, "选择授权码 Excel 文件", "",
            "Excel 文件 (*.xlsx)")
        if not filepath:
            return
        self.ui.lineEditAuthExcel.setText(filepath)
        self._auth_excel_path = filepath

        try:
            parser = AuthExcelParser()
            parser.load(filepath)
            total, used, remain = parser.get_stats()
            parser.close()
        except Exception as e:
            self.logger.error(f"加载 Excel 失败: {e}")
            self._auth_excel_path = ""
            self.ui.lineEditAuthExcel.clear()
            return

        self.ui.labelAuthTotal.setText(f"Total: {total}")
        self.ui.labelAuthUsed.setText(f"Used: {used}")
        self.ui.labelAuthRemain.setText(f"Remain: {remain}")
        self._authUpdateButtons()

        msg = QMessageBox(self)
        msg.setWindowTitle("Backup Reminder")
        msg.setText("授权过程会修改 Excel 文件写入状态标记，请确认已备份原文件。\n是否继续？")
        msg.setStandardButtons(QMessageBox.Yes | QMessageBox.No)
        if msg.exec() == QMessageBox.No:
            self._auth_excel_path = ""
            self.ui.lineEditAuthExcel.clear()
            self.ui.labelAuthTotal.setText("Total: 0")
            self.ui.labelAuthUsed.setText("Used: 0")
            self.ui.labelAuthRemain.setText("Remain: 0")
            self._authUpdateButtons()

    def _authAppendLog(self, level, line):
        color_map = {
            "ERROR": "#FF5555",
            "INFO": "#FFFFFF",
        }
        color = color_map.get(level, "#FFFFFF")
        if "成功" in line or "succeeds" in line.lower() or "通过" in line:
            color = "#55FF55"
        elif level == "ERROR":
            color = "#FF5555"
        self.ui.textBrowserAuthLog.append(
            f'<span style="color:{color};">{line}</span>')

    def _authSetConfigEnabled(self, enabled):
        self.ui.comboBoxAuthPort.setEnabled(enabled)
        self.ui.comboBoxAuthBaud.setEnabled(enabled)
        self.ui.comboBoxAuthChip.setEnabled(enabled)
        self.ui.pushButtonAuthBrowse.setEnabled(enabled)
        self.ui.pushButtonAuthRescan.setEnabled(enabled)
        self.ui.pushButtonAuthReadMAC.setEnabled(enabled)

    def _authStart(self):
        port = self.ui.comboBoxAuthPort.currentText()
        baud = int(self.ui.comboBoxAuthBaud.currentText())
        if not port or not self._auth_excel_path:
            return

        self._authSetConfigEnabled(False)
        self.ui.pushButtonAuthStart.setEnabled(False)
        self.ui.pushButtonAuthStop.setEnabled(True)
        self.ui.labelAuthStatus.setText("State: authorizing")
        self.ui.progressBarAuth.setValue(0)

        self._auth_handler = AuthHandler()
        self._auth_worker = AuthWorker(
            self._auth_handler, port, baud, self._auth_excel_path)
        self._auth_worker.log_signal.connect(self._authAppendLog)
        self._auth_worker.device_info_signal.connect(self._authUpdateDeviceInfo)
        self._auth_worker.stats_signal.connect(self._authUpdateStats)
        self._auth_worker.finished_signal.connect(self._authFinished)
        self._auth_worker.start()

    def _authStop(self):
        if self._auth_handler:
            self._auth_handler.stop()
        self.ui.labelAuthStatus.setText("State: stopping")

    def _authFinished(self, success):
        self._authSetConfigEnabled(True)
        self.ui.pushButtonAuthStop.setEnabled(False)
        self._authUpdateButtons()
        self.ui.progressBarAuth.setValue(100 if success else 0)

        if success:
            self.ui.labelAuthStatus.setText("State: success")
            self.ui.labelAuthStatus.setStyleSheet("color: #55FF55;")
        else:
            status_text = self.ui.labelAuthStatus.text()
            if "stopping" in status_text:
                self.ui.labelAuthStatus.setText("State: stopped")
                self.ui.labelAuthStatus.setStyleSheet("")
            else:
                self.ui.labelAuthStatus.setText("State: failed")
                self.ui.labelAuthStatus.setStyleSheet("color: #FF5555;")

        if self._auth_handler:
            self._auth_handler.cleanup()
            self._auth_handler = None
        self._auth_worker = None

    def _authUpdateDeviceInfo(self, mac, uuid, status):
        self.ui.labelAuthMAC.setText(f"MAC: {mac}")
        self.ui.labelAuthUUID.setText(f"UUID: {uuid}")
        status_map = {
            "reading": "State: reading",
            "writing": "State: writing",
            "success": "State: success",
            "already_authorized": "State: already authorized",
            "failed": "State: failed",
            "write_failed": "State: write failed",
            "verify_failed": "State: verify failed",
            "no_codes": "State: no codes left",
        }
        display = status_map.get(status, f"State: {status}")
        self.ui.labelAuthStatus.setText(display)
        if status == "success" or status == "already_authorized":
            self.ui.labelAuthStatus.setStyleSheet("color: #55FF55;")
        elif "fail" in status or status == "no_codes":
            self.ui.labelAuthStatus.setStyleSheet("color: #FF5555;")
        else:
            self.ui.labelAuthStatus.setStyleSheet("")

    def _authUpdateStats(self, total, used, remain):
        self.ui.labelAuthTotal.setText(f"Total: {total}")
        self.ui.labelAuthUsed.setText(f"Used: {used}")
        self.ui.labelAuthRemain.setText(f"Remain: {remain}")

    def _authReadMAC(self):
        port = self.ui.comboBoxAuthPort.currentText()
        if not port:
            return
        baud = int(self.ui.comboBoxAuthBaud.currentText())

        self.ui.pushButtonAuthReadMAC.setEnabled(False)
        self.ui.labelAuthMAC.setText("MAC: reading...")

        self._mac_worker = ReadMACWorker(port, baud)
        self._mac_worker.log_signal.connect(self._authAppendLog)
        self._mac_worker.mac_signal.connect(self._authOnMACRead)
        self._mac_worker.start()

    def _authOnMACRead(self, mac):
        if mac:
            self.ui.labelAuthMAC.setText(f"MAC: {mac}")
            self._authAppendLog("INFO",
                                f"[INFO] 设备 MAC: {mac}")
        else:
            self.ui.labelAuthMAC.setText("MAC: --")
            self._authAppendLog("ERROR",
                                f"[ERROR] 读取 MAC 失败: 设备无响应")
        self.ui.pushButtonAuthReadMAC.setEnabled(True)
        self._mac_worker = None
