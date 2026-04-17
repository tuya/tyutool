#!/usr/bin/env python
# -*- coding: utf-8 -*-

import os
import time
import logging
import threading

import serial
from serial.tools import list_ports

from PySide6 import QtWidgets
from PySide6.QtCore import Qt, QThread, Signal
from PySide6.QtWidgets import (QFileDialog, QFrame, QHBoxLayout, QLabel,
                                QMessageBox, QScrollArea, QVBoxLayout)

from tyutool.auth import AuthHandler, AuthExcelParser
from tyutool.flash import FlashArgv, FlashHandler, FlashInterface, ProgressHandler


# Step state constants
_STATE_PENDING = "pending"
_STATE_RUNNING = "running"
_STATE_DONE = "done"
_STATE_FAILED = "failed"

_STEP_NAMES = {
    AuthHandler.STEP_OPEN: "Open Serial",
    AuthHandler.STEP_FLASH: "Flash Firmware",
    AuthHandler.STEP_RESET: "Device Reset",
    AuthHandler.STEP_READ_MAC: "Read MAC",
    AuthHandler.STEP_AUTH: "Device Auth",
    AuthHandler.STEP_VERIFY: "Auth Verify",
    AuthHandler.STEP_CLOSE: "Close Serial",
}

_STEP_COLORS = {
    _STATE_PENDING: "#9ca3af",
    _STATE_RUNNING: "#2563eb",
    _STATE_DONE: "#16a34a",
    _STATE_FAILED: "#dc2626",
}

_STEP_ICONS = {
    _STATE_PENDING: "○",
    _STATE_RUNNING: "●",
    _STATE_DONE: "✔",
    _STATE_FAILED: "✘",
}

# Flash can fail transiently on serial/sync; retry before giving up.
_FLASH_MAX_ATTEMPTS = 10

_STEP_BG = {
    _STATE_PENDING: "#f3f4f6",
    _STATE_RUNNING: "#eff6ff",
    _STATE_DONE: "#f0fdf4",
    _STATE_FAILED: "#fef2f2",
}


class _StepItem:
    """One row in the workflow checklist (icon + name + detail)."""

    def __init__(self, number, step_id, parent=None):
        self.step_id = step_id

        self.frame = QFrame(parent)
        self.frame.setObjectName("authStepFrame")

        layout = QHBoxLayout(self.frame)
        layout.setContentsMargins(10, 4, 10, 4)
        layout.setSpacing(8)

        self.icon_label = QLabel()
        self.icon_label.setFixedWidth(22)
        self.icon_label.setAlignment(Qt.AlignmentFlag.AlignCenter)

        self.name_label = QLabel(f"{number}. {_STEP_NAMES[step_id]}")

        self.detail_label = QLabel()
        self.detail_label.setAlignment(
            Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)

        layout.addWidget(self.icon_label)
        layout.addWidget(self.name_label, 1)
        layout.addWidget(self.detail_label)

        self.set_state(_STATE_PENDING)

    def set_state(self, state, detail=None):
        color = _STEP_COLORS[state]
        bg = _STEP_BG[state]
        icon = _STEP_ICONS[state]

        self.frame.setStyleSheet(
            f"QFrame#authStepFrame {{"
            f" background-color: {bg};"
            f" border-left: 3px solid {color};"
            f" border-radius: 6px;"
            f"}}"
            f" QLabel {{ background: transparent; border: none; }}")

        self.icon_label.setText(icon)
        self.icon_label.setStyleSheet(f"font-size: 16px; color: {color};")

        if state == _STATE_RUNNING:
            self.name_label.setStyleSheet(
                f"font-weight: bold; color: {color};")
        elif state == _STATE_PENDING:
            self.name_label.setStyleSheet("color: #6b7280;")
        else:
            self.name_label.setStyleSheet("color: #374151;")

        if detail is not None:
            self.detail_label.setText(detail)
        self.detail_label.setStyleSheet("color: #9ca3af; font-size: 12px;")

    def destroy(self):
        self.frame.setParent(None)
        self.frame.deleteLater()


class _LogProgressHandler(ProgressHandler):
    """Progress handler that reports flash progress via a log callback."""

    def __init__(self, log_callback, step_callback=None):
        super().__init__()
        self._log = log_callback
        self._step = step_callback
        self._phase = ""
        self._total = 0
        self._current = 0
        self._last_pct = -1

    def setup(self, header, total):
        self._phase = header
        self._total = total
        self._current = 0
        self._last_pct = -1
        self._log("INFO", f"Flash {header}: total={total}")
        if self._step:
            self._step(AuthHandler.STEP_FLASH, _STATE_RUNNING,
                       f"{header} 0%")

    def start(self):
        self._current = 0

    def update(self, size=1):
        self._current += size
        if self._total > 0:
            pct = min(100, int(self._current * 100 / self._total))
            if pct // 10 > self._last_pct // 10:
                self._last_pct = pct
                self._log("INFO", f"Flash {self._phase}: {pct}%")
                if self._step:
                    self._step(AuthHandler.STEP_FLASH, _STATE_RUNNING,
                               f"{self._phase} {pct}%")

    def close(self):
        pass


class AuthWorker(QThread):
    """Background thread that optionally flashes firmware, then runs authorization."""
    log_signal = Signal(str, str)
    step_signal = Signal(str, str, str)
    log_path_signal = Signal(str)
    device_info_signal = Signal(str, str, str)
    stats_signal = Signal(int, int, int)
    finished_signal = Signal(bool)
    confirm_signal = Signal(str, str)

    def __init__(self, handler, port, baudrate, excel_path,
                 firmware_path="", chip="", reuse_log_path=None):
        super().__init__()
        self.handler = handler
        self.port = port
        self.baudrate = baudrate
        self.excel_path = excel_path
        self.firmware_path = firmware_path
        self.chip = chip
        self.reuse_log_path = reuse_log_path
        self._confirm_event = threading.Event()
        self._confirm_result = False

    def _on_confirm(self, title, message):
        """Block worker thread until the main thread answers the dialog."""
        self._confirm_event.clear()
        self._confirm_result = False
        self.confirm_signal.emit(title, message)
        self._confirm_event.wait()
        return self._confirm_result

    def set_confirm_result(self, result):
        """Called from the main thread after user responds to the dialog."""
        self._confirm_result = result
        self._confirm_event.set()

    def _flash_firmware(self):
        """Flash firmware before authorization. Returns True on success."""
        self.handler.auth_log.info(f"Flashing firmware: {self.firmware_path}")

        flash_baud = FlashInterface.get_baudrate(self.chip) or 921600
        start_addr = FlashInterface.get_start_addr(self.chip) or 0x00
        argv = FlashArgv("Write", self.chip, self.port, flash_baud,
                         start_addr, self.firmware_path)

        handler_cls = FlashInterface.get_flash_handler(self.chip)
        if not handler_cls:
            self.handler.auth_log.error(f"Unsupported chip: {self.chip}")
            return False

        def _log_to_file(lvl, line):
            if self.handler.auth_log:
                self.handler.auth_log.log(lvl, line)

        for attempt in range(1, _FLASH_MAX_ATTEMPTS + 1):
            if attempt > 1:
                self.handler.auth_log.info(
                    f"Flash retry {attempt}/{_FLASH_MAX_ATTEMPTS}...")
                time.sleep(0.5)

            progress = _LogProgressHandler(
                _log_to_file,
                step_callback=lambda sid, st, det: self.step_signal.emit(sid, st, det))
            logger = logging.getLogger("auth_flash")
            soc_handler = handler_cls(argv, logger=logger, progress=progress)
            soc_handler.start()

            try:
                if not soc_handler.shake():
                    self.handler.auth_log.error(
                        f"Flash handshake failed "
                        f"(attempt {attempt}/{_FLASH_MAX_ATTEMPTS})")
                    if attempt >= _FLASH_MAX_ATTEMPTS:
                        return False
                    continue
                if not soc_handler.erase():
                    self.handler.auth_log.error(
                        f"Flash erase failed "
                        f"(attempt {attempt}/{_FLASH_MAX_ATTEMPTS})")
                    if attempt >= _FLASH_MAX_ATTEMPTS:
                        return False
                    continue
                if not soc_handler.write():
                    self.handler.auth_log.error(
                        f"Flash write failed "
                        f"(attempt {attempt}/{_FLASH_MAX_ATTEMPTS})")
                    if attempt >= _FLASH_MAX_ATTEMPTS:
                        return False
                    continue
                soc_handler.crc_check()
                soc_handler.reboot()
                self.handler.auth_log.info(
                    "Flash completed, waiting for device boot...")
                return True
            except Exception as e:
                self.handler.auth_log.error(
                    f"Flash error: {e} "
                    f"(attempt {attempt}/{_FLASH_MAX_ATTEMPTS})")
                if attempt >= _FLASH_MAX_ATTEMPTS:
                    return False
            finally:
                soc_handler.serial_close()
        return False

    def _reset_device(self):
        """Reset device via DTR/RTS toggle after firmware flash."""
        try:
            self.handler.auth_log.info("Resetting device via DTR/RTS...")
            ser = serial.Serial(port=self.port, baudrate=self.baudrate)
            ser.dtr = False
            ser.rts = True
            time.sleep(0.1)
            ser.rts = False
            time.sleep(0.1)
            ser.close()
            self.handler.auth_log.info("Device reset signal sent")
        except Exception as e:
            self.handler.auth_log.error(f"Reset via DTR/RTS failed: {e}")

    def run(self):
        self.handler.on_log = lambda level, line: self.log_signal.emit(level, line)
        self.handler.on_device_info = lambda mac, uuid, st: self.device_info_signal.emit(mac, uuid, st)
        self.handler.on_stats = lambda t, u, r: self.stats_signal.emit(t, u, r)
        self.handler.on_step = lambda sid, st, det: self.step_signal.emit(sid, st, det)
        self.handler.on_confirm = self._on_confirm

        try:
            log_path = self.handler.init_log(
                self.excel_path, reuse_log_path=self.reuse_log_path)
            self.handler.auth_log.info("--- Authorization run started ---")
            self.log_path_signal.emit(log_path)
        except Exception as e:
            self.log_signal.emit("ERROR", f"Failed to create log: {e}")
            self.finished_signal.emit(False)
            return

        has_firmware = bool(self.firmware_path and self.chip)

        try:
            if has_firmware:
                # Step 1: verify serial port
                self.step_signal.emit(AuthHandler.STEP_OPEN, _STATE_RUNNING, "")
                try:
                    ser = serial.Serial(
                        port=self.port, baudrate=self.baudrate, timeout=1)
                    ser.close()
                except Exception as e:
                    self.handler.auth_log.error(f"Failed to open serial: {e}")
                    self.step_signal.emit(
                        AuthHandler.STEP_OPEN, _STATE_FAILED, str(e))
                    self.finished_signal.emit(False)
                    return
                self.handler.auth_log.info(
                    f"Serial port verified: {self.port}")
                self.step_signal.emit(AuthHandler.STEP_OPEN, _STATE_DONE, "")

                # Step 2: flash firmware
                self.step_signal.emit(
                    AuthHandler.STEP_FLASH, _STATE_RUNNING, "")
                if not self._flash_firmware():
                    self.step_signal.emit(
                        AuthHandler.STEP_FLASH, _STATE_FAILED, "")
                    self.finished_signal.emit(False)
                    return
                self.step_signal.emit(AuthHandler.STEP_FLASH, _STATE_DONE, "")

                # Step 3: reset device via DTR/RTS and wait for boot
                self.step_signal.emit(
                    AuthHandler.STEP_RESET, _STATE_RUNNING, "")
                self._reset_device()
                time.sleep(0.5)
                self.handler.auth_log.info(
                    "Device booted, starting authorization...")
                self.step_signal.emit(
                    AuthHandler.STEP_RESET, _STATE_DONE, "")

            # Remaining steps (read MAC, auth, verify);
            # skip_open=True when firmware was flashed (STEP_OPEN already done)
            result = self.handler.authorize_single(
                self.port, self.baudrate, self.excel_path,
                skip_open=has_firmware)
        except Exception as e:
            if self.handler.auth_log:
                self.handler.auth_log.error(f"Authorization error: {e}")
            result = False
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


class BatchAuthGUI(QtWidgets.QMainWindow):
    _AUTH_CHIPS = ["ESP32"]

    def __init__(self):
        super().__init__()
        self._auth_handler = None
        self._auth_worker = None
        self._mac_worker = None
        self._auth_excel_path = ""
        self._auth_firmware_path = ""
        self._auth_steps = []
        self._auth_step_items = []
        self._auth_log_path = ""
        self._auth_session_log_path = ""

    def authUiSetup(self):
        self.ui.comboBoxAuthChip.addItems(self._AUTH_CHIPS)
        self.ui.comboBoxAuthBaud.addItems(["115200", "230400", "460800", "921600"])

        self.ui.pushButtonAuthStart.setEnabled(False)
        self.ui.pushButtonAuthReadMAC.setEnabled(False)

        self.ui.labelAuthDeviceHeader.hide()

        for lbl in (self.ui.labelAuthMAC, self.ui.labelAuthUUID):
            lbl.setCursor(Qt.CursorShape.PointingHandCursor)
            lbl.setToolTip("Click to copy")
        self.ui.labelAuthMAC.mousePressEvent = (
            lambda _e: self._authCopyLabelValue(self.ui.labelAuthMAC))
        self.ui.labelAuthUUID.mousePressEvent = (
            lambda _e: self._authCopyLabelValue(self.ui.labelAuthUUID))

        self.ui.textBrowserAuthLog.hide()

        scroll = QScrollArea(self.ui.tabAuth)
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.Shape.NoFrame)
        scroll.setHorizontalScrollBarPolicy(
            Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self._auth_step_container = QtWidgets.QWidget()
        self._auth_step_vlayout = QVBoxLayout(self._auth_step_container)
        self._auth_step_vlayout.setContentsMargins(4, 4, 4, 4)
        self._auth_step_vlayout.setSpacing(3)
        scroll.setWidget(self._auth_step_container)
        self.ui.verticalLayout_auth_root.addWidget(scroll)

        self._auth_log_label = QLabel(self._auth_step_container)
        self._auth_log_label.setWordWrap(True)
        self._auth_log_label.setCursor(Qt.CursorShape.PointingHandCursor)
        self._auth_log_label.setToolTip("Click to copy path")
        self._auth_log_label.setStyleSheet(
            "color: #6b7280; font-size: 12px; padding: 6px 10px;")
        self._auth_log_label.mousePressEvent = self._authCopyLogPath
        self._auth_log_label.hide()

        self._authInitSteps(has_firmware=False)

        self.ui.pushButtonAuthRescan.clicked.connect(self._authRescanPorts)
        self.ui.pushButtonAuthBrowseFirmware.clicked.connect(self._authBrowseFirmware)
        self.ui.pushButtonAuthBrowse.clicked.connect(self._authBrowseExcel)
        self.ui.pushButtonAuthStart.clicked.connect(self._authStart)
        self.ui.pushButtonAuthReadMAC.clicked.connect(self._authReadMAC)
        self.ui.comboBoxAuthPort.currentTextChanged.connect(self._authUpdateButtons)

        self._authRescanPorts()

    # ── Step workflow helpers ──────────────────────────────────────

    def _authInitSteps(self, has_firmware):
        """Build the ordered step list and create step widgets."""
        for item in self._auth_step_items:
            item.destroy()
        self._auth_step_items = []
        while self._auth_step_vlayout.count() > 0:
            self._auth_step_vlayout.takeAt(0)

        ids = [AuthHandler.STEP_OPEN]
        if has_firmware:
            ids += [AuthHandler.STEP_FLASH, AuthHandler.STEP_RESET]
        ids += [AuthHandler.STEP_READ_MAC, AuthHandler.STEP_AUTH,
                AuthHandler.STEP_VERIFY, AuthHandler.STEP_CLOSE]
        self._auth_steps = [[sid, _STATE_PENDING, ""] for sid in ids]

        for i, (sid, _, _) in enumerate(self._auth_steps, 1):
            item = _StepItem(i, sid, self._auth_step_container)
            self._auth_step_vlayout.addWidget(item.frame)
            self._auth_step_items.append(item)

        self._auth_step_vlayout.addWidget(self._auth_log_label)
        self._auth_step_vlayout.addStretch(1)
        self._auth_log_path = ""
        self._auth_log_label.hide()

        self.ui.progressBarAuth.setRange(0, len(self._auth_steps))
        self.ui.progressBarAuth.setValue(0)

    def _authResetSteps(self):
        """Reset all steps to pending state for a new auth run."""
        for i, entry in enumerate(self._auth_steps):
            entry[1] = _STATE_PENDING
            entry[2] = ""
            if i < len(self._auth_step_items):
                self._auth_step_items[i].set_state(_STATE_PENDING, "")
        self._auth_log_path = ""
        self._auth_log_label.hide()
        self.ui.progressBarAuth.setValue(0)

    def _authOnStep(self, step_id, state, detail):
        """Slot for step_signal: update step widget directly."""
        for i, entry in enumerate(self._auth_steps):
            if entry[0] == step_id:
                entry[1] = state
                if detail:
                    entry[2] = detail
                if i < len(self._auth_step_items):
                    self._auth_step_items[i].set_state(
                        state, detail if detail else None)
                break
        self._authUpdateProgress()

    def _authUpdateProgress(self):
        """Update progress bar based on completed / total steps."""
        total = len(self._auth_steps)
        if total == 0:
            return
        done = sum(1 for _, st, _ in self._auth_steps
                   if st in (_STATE_DONE, _STATE_FAILED))
        self.ui.progressBarAuth.setRange(0, total)
        self.ui.progressBarAuth.setValue(done)

    def _authOnLogPath(self, path):
        self._auth_log_path = path
        if not self._auth_session_log_path:
            self._auth_session_log_path = path
        self._auth_log_label.setText(f"Full log: {path}")
        self._auth_log_label.show()

    def _authCopyLogPath(self, _event):
        if self._auth_log_path:
            QtWidgets.QApplication.clipboard().setText(self._auth_log_path)
            self._auth_log_label.setToolTip("Copied!")
            self._auth_log_label.setText(f"Full log (copied): {self._auth_log_path}")

    def _authCopyLabelValue(self, label):
        text = label.text()
        prefix_end = text.find(": ")
        value = text[prefix_end + 2:].strip() if prefix_end >= 0 else ""
        if not value or value in ("--", "reading..."):
            return
        QtWidgets.QApplication.clipboard().setText(value)
        label.setToolTip("Copied!")

    # ── Port / file helpers ───────────────────────────────────────

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

    def _authBrowseFirmware(self):
        filepath, _ = QFileDialog.getOpenFileName(
            self, "Select Firmware File", "",
            "Firmware Files (*.bin)")
        if not filepath:
            return
        if not os.path.isfile(filepath):
            return
        self.ui.lineEditAuthFirmware.setText(filepath)
        self._auth_firmware_path = filepath
        self._authInitSteps(has_firmware=True)

    def _authBrowseExcel(self):
        filepath, _ = QFileDialog.getOpenFileName(
            self, "Select License Excel File", "",
            "Excel Files (*.xlsx)")
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
            self._auth_excel_path = ""
            self.ui.lineEditAuthExcel.clear()
            QMessageBox.warning(self, "Error",
                                f"Failed to load Excel:\n{e}")
            return

        self.ui.labelAuthTotal.setText(f"Total: {total}")
        self.ui.labelAuthUsed.setText(f"Used: {used}")
        self.ui.labelAuthRemain.setText(f"Remain: {remain}")
        self._authUpdateButtons()

        msg = QMessageBox(self)
        msg.setWindowTitle("Backup Reminder")
        msg.setText("The authorization process will modify the Excel file "
                    "to mark status.\nPlease confirm you have backed up "
                    "the original file.\nContinue?")
        msg.setStandardButtons(QMessageBox.Yes | QMessageBox.No)
        if msg.exec() == QMessageBox.No:
            self._auth_excel_path = ""
            self.ui.lineEditAuthExcel.clear()
            self.ui.labelAuthTotal.setText("Total: 0")
            self.ui.labelAuthUsed.setText("Used: 0")
            self.ui.labelAuthRemain.setText("Remain: 0")
            self._authUpdateButtons()
        else:
            import shutil
            bak_path = filepath + ".bak"
            if not os.path.exists(bak_path):
                try:
                    shutil.copy2(filepath, bak_path)
                except Exception:
                    pass

    # ── Config enable / disable ───────────────────────────────────

    def _authSetConfigEnabled(self, enabled):
        self.ui.comboBoxAuthPort.setEnabled(enabled)
        self.ui.comboBoxAuthChip.setEnabled(enabled)
        self.ui.lineEditAuthFirmware.setEnabled(enabled)
        self.ui.pushButtonAuthBrowseFirmware.setEnabled(enabled)
        self.ui.comboBoxAuthBaud.setEnabled(enabled)
        self.ui.pushButtonAuthBrowse.setEnabled(enabled)
        self.ui.pushButtonAuthRescan.setEnabled(enabled)
        self.ui.pushButtonAuthReadMAC.setEnabled(enabled)

    # ── Start / Finished ───────────────────────────────────────────

    def _authStart(self):
        port = self.ui.comboBoxAuthPort.currentText()
        baud = int(self.ui.comboBoxAuthBaud.currentText())
        if not port or not self._auth_excel_path:
            return

        firmware = self._auth_firmware_path
        chip = self.ui.comboBoxAuthChip.currentText()
        if firmware and not os.path.isfile(firmware):
            QMessageBox.warning(self, "Error",
                                f"Firmware file not found:\n{firmware}")
            return

        self._authSetConfigEnabled(False)
        self.ui.pushButtonAuthStart.setEnabled(False)
        self.ui.labelAuthStatus.setText("State: authorizing")
        self.ui.labelAuthStatus.setStyleSheet("")

        self._authResetSteps()

        self._auth_handler = AuthHandler()
        reuse_log = self._auth_session_log_path or None
        self._auth_worker = AuthWorker(
            self._auth_handler, port, baud, self._auth_excel_path,
            firmware_path=firmware, chip=chip, reuse_log_path=reuse_log)
        self._auth_worker.step_signal.connect(self._authOnStep)
        self._auth_worker.log_path_signal.connect(self._authOnLogPath)
        self._auth_worker.device_info_signal.connect(self._authUpdateDeviceInfo)
        self._auth_worker.stats_signal.connect(self._authUpdateStats)
        self._auth_worker.confirm_signal.connect(self._authOnConfirm)
        self._auth_worker.finished_signal.connect(self._authFinished)
        self._auth_worker.start()

    def _authOnConfirm(self, title, message):
        """Show a Yes/No/Copy dialog on the main thread, then unblock the worker."""
        msg_box = QMessageBox(self)
        msg_box.setWindowTitle(title)
        msg_box.setText(message)
        msg_box.setIcon(QMessageBox.Icon.Question)

        yes_btn = msg_box.addButton(QMessageBox.StandardButton.Yes)
        no_btn = msg_box.addButton(QMessageBox.StandardButton.No)
        copy_btn = msg_box.addButton("Copy", QMessageBox.ButtonRole.ActionRole)
        msg_box.setDefaultButton(no_btn)

        while True:
            msg_box.exec()
            if msg_box.clickedButton() == copy_btn:
                QtWidgets.QApplication.clipboard().setText(message)
                continue
            break

        result = msg_box.clickedButton() == yes_btn
        if self._auth_worker:
            self._auth_worker.set_confirm_result(result)

    def _authFinished(self, success):
        self._authSetConfigEnabled(True)
        self._authUpdateButtons()

        total = len(self._auth_steps)
        self.ui.progressBarAuth.setRange(0, total)
        self.ui.progressBarAuth.setValue(total if success else
                                         self.ui.progressBarAuth.value())

        status_text = self.ui.labelAuthStatus.text()
        if success:
            if "already authorized" in status_text:
                self.ui.labelAuthStatus.setStyleSheet("color: #16a34a;")
            elif "skipped" in status_text:
                self.ui.labelAuthStatus.setStyleSheet("color: #d97706;")
            else:
                self.ui.labelAuthStatus.setText("State: success")
                self.ui.labelAuthStatus.setStyleSheet("color: #16a34a;")
        else:
            self.ui.labelAuthStatus.setText("State: failed")
            self.ui.labelAuthStatus.setStyleSheet("color: #dc2626;")

        if self._auth_handler:
            self._auth_handler.cleanup()
            self._auth_handler = None
        self._auth_worker = None

    # ── Device info / stats ───────────────────────────────────────

    def _authUpdateDeviceInfo(self, mac, uuid, status):
        self.ui.labelAuthMAC.setText(f"MAC: {mac}")
        self.ui.labelAuthUUID.setText(f"UUID: {uuid}")
        status_map = {
            "reading": "State: reading",
            "writing": "State: writing",
            "success": "State: success",
            "already_authorized": "State: already authorized",
            "skipped": "State: skipped (user chose to skip)",
            "auth_mismatch": "State: auth mismatch",
            "failed": "State: failed",
            "write_failed": "State: write failed",
            "verify_failed": "State: verify failed",
            "no_codes": "State: no codes left",
        }
        display = status_map.get(status, f"State: {status}")
        self.ui.labelAuthStatus.setText(display)
        if status in ("success", "already_authorized"):
            self.ui.labelAuthStatus.setStyleSheet("color: #16a34a;")
        elif status in ("skipped", "auth_mismatch"):
            self.ui.labelAuthStatus.setStyleSheet("color: #d97706;")
        elif "fail" in status or status == "no_codes":
            self.ui.labelAuthStatus.setStyleSheet("color: #dc2626;")
        else:
            self.ui.labelAuthStatus.setStyleSheet("")

    def _authUpdateStats(self, total, used, remain):
        self.ui.labelAuthTotal.setText(f"Total: {total}")
        self.ui.labelAuthUsed.setText(f"Used: {used}")
        self.ui.labelAuthRemain.setText(f"Remain: {remain}")

    # ── Read MAC (standalone) ─────────────────────────────────────

    def _authReadMAC(self):
        port = self.ui.comboBoxAuthPort.currentText()
        if not port:
            return
        baud = int(self.ui.comboBoxAuthBaud.currentText())

        self.ui.pushButtonAuthReadMAC.setEnabled(False)
        self.ui.labelAuthMAC.setText("MAC: reading...")

        self._mac_worker = ReadMACWorker(port, baud)
        self._mac_worker.mac_signal.connect(self._authOnMACRead)
        self._mac_worker.start()

    def _authOnMACRead(self, mac):
        if mac:
            self.ui.labelAuthMAC.setText(f"MAC: {mac}")
        else:
            self.ui.labelAuthMAC.setText("MAC: --")
        self.ui.pushButtonAuthReadMAC.setEnabled(True)
        self._mac_worker = None
