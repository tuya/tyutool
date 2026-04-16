#!/usr/bin/env python
# -*- coding: utf-8 -*-

import os
import sys
import base64
import logging
import logging.handlers
import threading

# Debug log helper — writes to tyutool_debug.log next to the executable
_debug_log_path = os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])),
                               "tyutool_debug.log")


def _dbg(msg):
    try:
        with open(_debug_log_path, 'a') as f:
            f.write(f"[main] {msg}\n")
            f.flush()
    except Exception:
        pass


_dbg("main.py module loading...")

from PySide6 import QtCore, QtWidgets, QtGui
# from PySide6.QtWidgets import QFileDialog
from PySide6.QtGui import QAction, QIcon, QPixmap
from PySide6.QtCore import QTimer, QThread, Signal
from PySide6.QtWidgets import QApplication, QMessageBox

from .ui_main import Ui_MainWindow
from .ui_logo import LOGO_ICON_BYTES

from tyutool.gui.flash import FlashGUI
from tyutool.gui.serial import SerialGUI
from tyutool.gui.ser_debug import SerDebugGUI
from tyutool.gui.web_debug import WebDebugGUI
from tyutool.gui.auth import BatchAuthGUI
import requests
from tyutool.util import TyutoolUpgrade
from tyutool.util.util import tyutool_root, set_logger, TYUTOOL_VERSION, tyutool_version, network_available, get_country_code

# Terms & agreements (Batch Auth / open-source notice)
_ABOUT_TERMS_URL = (
    "https://github.com/tuya/tyutool/blob/feat/tuyaopen-auth/Terms_And_Agreements.md"
)


class EmittingStr(QtCore.QObject):
    textWritten = QtCore.Signal(str)

    def write(self, text):
        # Use QMetaObject.invokeMethod to safely emit from any thread
        self.textWritten.emit(str(text))

    def flush(self):
        pass


class _AskUpgradeSignal(QtCore.QObject):
    """Helper QObject to safely deliver results from a background thread to the main thread."""
    should_upgrade = Signal(bool, str)


def _ask_upgrade_worker(signal_obj):
    """Run upgrade check in a plain thread (avoids QThread GC issues)."""
    try:
        is_script = '.py' in sys.argv[0]
        print(f"[DEBUG] AskUpgrade: is_script={is_script}", flush=True)
        if is_script:
            signal_obj.should_upgrade.emit(False, "")
            return

        if not network_available():
            print(f"[DEBUG] AskUpgrade: network not available", flush=True)
            signal_obj.should_upgrade.emit(False, "")
            return

        print(f"[DEBUG] AskUpgrade: getting country code...", flush=True)
        country = get_country_code()
        print(f"[DEBUG] AskUpgrade: country={country}", flush=True)
        if country == "China":
            api_url = "https://gitee.com/api/v5/repos/tuya-open/tyutool/releases/latest"
        else:
            api_url = "https://api.github.com/repos/tuya/tyutool/releases/latest"

        try:
            response = requests.get(api_url, timeout=5)
            data = response.json()
        except Exception:
            signal_obj.should_upgrade.emit(False, "")
            return

        server_version = data.get('tag_name', "").lstrip('v')
        if not server_version:
            signal_obj.should_upgrade.emit(False, "")
            return

        current_version = tyutool_version()
        if server_version <= current_version:
            signal_obj.should_upgrade.emit(False, "")
            return

        signal_obj.should_upgrade.emit(True, server_version)
        print(f"[DEBUG] AskUpgrade: should upgrade to {server_version}", flush=True)
    except Exception as e:
        print(f"[DEBUG] AskUpgrade: exception {e}", flush=True)
        signal_obj.should_upgrade.emit(False, "")


class UpgradeThread(QThread):
    progress_updated = Signal(int)
    upgrade_finished = Signal(bool, str)

    def __init__(self, logger, running_type):
        super().__init__()
        self.logger = logger
        self.running_type = running_type

    def run(self):
        try:
            def progress_callback(block_num, block_size, total_size):
                if total_size > 0:
                    downloaded = block_num * block_size
                    progress = min(100, int((downloaded / total_size) * 100))
                    self.progress_updated.emit(progress)

            def exit_callback():
                # exit by GUI self
                pass

            up_handle = TyutoolUpgrade(self.logger, self.running_type,
                                       show_progress=progress_callback,
                                       exit_callback=exit_callback)

            success = up_handle.upgrade()
            if success:
                self.upgrade_finished.emit(True, "Upgrade success.")
            else:
                self.upgrade_finished.emit(False, "Upgrade failed.")
        except Exception as e:
            self.upgrade_finished.emit(False, str(e))


class MyWidget(FlashGUI, SerialGUI, SerDebugGUI, WebDebugGUI, BatchAuthGUI):
    def __init__(self):
        _dbg("MyWidget.__init__ start")
        super().__init__()

        # log to textBrowser or to terminal
        sys.stderr = EmittingStr()
        sys.stderr.textWritten.connect(self.outputWritten)

        ui = Ui_MainWindow()
        ui.setupUi(self)
        self.ui = ui

        # About → README disclaimer: menu bar top-right (QWidgetAction is often
        # hidden when the OS/global menu exports menus; corner widget + in-window bar is reliable)
        mb = self.ui.menubar
        self._about_tool_btn = QtWidgets.QToolButton(mb)
        self._about_tool_btn.setObjectName("toolButtonAbout")
        self._about_tool_btn.setText("About")
        self._about_tool_btn.setAutoRaise(True)
        self._about_tool_btn.setToolButtonStyle(
            QtCore.Qt.ToolButtonStyle.ToolButtonTextOnly
        )
        self._about_tool_btn.setFocusPolicy(QtCore.Qt.FocusPolicy.NoFocus)
        self._about_tool_btn.setCursor(QtGui.QCursor(QtCore.Qt.CursorShape.PointingHandCursor))
        self._about_tool_btn.setStyleSheet(
            "QToolButton { color: palette(link); border: none; padding: 0 8px; }"
            "QToolButton:hover { text-decoration: underline; }"
        )
        self._about_tool_btn.clicked.connect(self._open_about_terms_url)

        # cache
        self.cache_dir = os.path.join(tyutool_root(), "cache")
        try:
            os.makedirs(self.cache_dir, exist_ok=True)
        except OSError:
            # Fallback to temp directory if the executable directory is not writable
            import tempfile
            self.cache_dir = os.path.join(tempfile.gettempdir(), "tyutool_cache")
            os.makedirs(self.cache_dir, exist_ok=True)

        # ico
        logo_icon = base64.b64decode(LOGO_ICON_BYTES)
        icon_pixmap = QPixmap()
        icon_pixmap.loadFromData(logo_icon)
        self.setWindowIcon(QIcon(icon_pixmap))

        self.logger = set_logger(logging.INFO)
        self.logger.info("Show Tuya Uart Tool.")

        _dbg("flashUiSetup...")
        self.flashUiSetup()
        _dbg("serialUiSetup...")
        self.serialUiSetup()
        _dbg("serDebugUiSetup...")
        self.serDebugUiSetup()
        _dbg("webDebugUiSetup...")
        self.webDebugUiSetup()
        _dbg("authUiSetup...")
        self.authUiSetup()
        _dbg("UI setup done")

        self.ui.menuDebug.triggered[QAction].connect(self.LogDebugSwitch)
        self.ui.actionUpgrade.triggered.connect(self.guiUpgrade)
        self.ui.actionVersion.triggered.connect(self.showVersion)

        self.upgrade_thread = None
        self._ask_upgrade_signal = None
        self._ask_upgrade_thread = None

        QTimer.singleShot(500, self.guiAskUpgrade)

        pass

    def _open_about_terms_url(self):
        QtGui.QDesktopServices.openUrl(QtCore.QUrl(_ABOUT_TERMS_URL))

    def _apply_about_menubar_corner(self):
        btn = getattr(self, "_about_tool_btn", None)
        if btn is None:
            return
        mb = self.ui.menubar
        h = max(24, mb.height(), mb.sizeHint().height())
        btn.setFixedHeight(h)
        btn.setMinimumWidth(48)
        mb.setCornerWidget(btn, QtCore.Qt.Corner.TopRightCorner)

    def showEvent(self, event):
        super().showEvent(event)
        self._apply_about_menubar_corner()

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self._apply_about_menubar_corner()

    def outputWritten(self, text):
        cursor = self.ui.textBrowserShow.textCursor()
        cursor.movePosition(QtGui.QTextCursor.End)
        cursor.insertText(text)
        self.ui.textBrowserShow.setTextCursor(cursor)
        self.ui.textBrowserShow.ensureCursorVisible()
        pass

    def _wait_threads(self):
        """Wait for all background threads to finish."""
        for attr in ('upgrade_thread', 'pic_loader', '_auth_worker', '_mac_worker'):
            thread = getattr(self, attr, None)
            if thread and thread.isRunning():
                print(f"[DEBUG] _wait_threads: waiting for {attr}", flush=True)
                thread.requestInterruption()
                thread.quit()
                thread.wait(5000)
        # Wait for the plain threading.Thread used by upgrade check
        t = getattr(self, '_ask_upgrade_thread', None)
        if t and t.is_alive():
            print("[DEBUG] _wait_threads: waiting for _ask_upgrade_thread", flush=True)
            t.join(timeout=5)
        for t in getattr(self, '_old_pic_loaders', []):
            if t.isRunning():
                t.requestInterruption()
                t.quit()
                t.wait(3000)

    def closeEvent(self, event):
        print(f"[DEBUG] closeEvent: cleaning up threads...", flush=True)
        self._wait_threads()
        print(f"[DEBUG] closeEvent: done", flush=True)
        super().closeEvent(event)

    def LogDebugSwitch(self, q):
        log_level = logging.DEBUG
        is_on = True
        if q.text() == "Off":
            log_level = logging.INFO
            is_on = False
        self.ui.actionOnDebug.setEnabled(not is_on)
        self.ui.actionOffDebug.setEnabled(is_on)
        set_logger(log_level)
        self.logger.info(f'Debug Mode: {is_on}')
        pass

    def guiUpgrade(self):
        self.startUpgradeWithProgress()
        pass

    def updateProgress(self, progress):
        self.ui.progressBarShow.setValue(progress)

    def upgradeFinished(self, success, message):
        if success:
            self.logger.info(message)
            # close GUI, delay 1s
            QTimer.singleShot(1000, lambda: QApplication.instance().quit())
        else:
            self.logger.error(message)

        self.ui.pushButtonStart.setEnabled(True)
        self.ui.pushButtonStop.setEnabled(True)
        self.ui.progressBarShow.setVisible(False)
        self.upgrade_thread = None

    def guiAskUpgrade(self):
        self._ask_upgrade_signal = _AskUpgradeSignal()
        self._ask_upgrade_signal.should_upgrade.connect(self._onAskUpgradeResult)
        self._ask_upgrade_thread = threading.Thread(
            target=_ask_upgrade_worker,
            args=(self._ask_upgrade_signal,),
            daemon=True
        )
        self._ask_upgrade_thread.start()

    def _onAskUpgradeResult(self, should_upgrade, server_version):
        self._ask_upgrade_signal = None
        self._ask_upgrade_thread = None
        if not should_upgrade:
            return

        skip_version_file = os.path.join(tyutool_root(), "cache", "skip_version.cache")
        skip_version = "0.0.0"
        if os.path.exists(skip_version_file):
            import json
            with open(skip_version_file, 'r', encoding='utf-8') as f:
                json_data = json.load(f)
                skip_version = json_data.get("version", "0.0.0")
        if skip_version >= server_version:
            return

        msg = QMessageBox(self)
        msg.setWindowTitle("Upgrade")
        msg.setText(f"Upgrade Tyutool to [{server_version}] ?")
        confirm_button = msg.addButton("Ok", QMessageBox.ActionRole)
        skip_button = msg.addButton("Skip", QMessageBox.ActionRole)
        cancel_button = msg.addButton("Cancel", QMessageBox.RejectRole)
        msg.exec()

        if msg.clickedButton() == confirm_button:
            QTimer.singleShot(100, self.startUpgradeWithProgress)
        elif msg.clickedButton() == skip_button:
            import json
            skip_version_data = {"version": server_version}
            os.makedirs(os.path.dirname(skip_version_file), exist_ok=True)
            with open(skip_version_file, 'w', encoding='utf-8') as f:
                json.dump(skip_version_data, f, indent=4, ensure_ascii=False)

    def startUpgradeWithProgress(self):
        if self.upgrade_thread and self.upgrade_thread.isRunning():
            self.logger.info("Upgrade running ...")
            return

        # progress
        self.ui.progressBarShow.setMaximum(100)
        self.ui.progressBarShow.setValue(0)
        self.ui.progressBarShow.setVisible(True)

        # Start and Stop
        self.ui.pushButtonStart.setEnabled(False)
        self.ui.pushButtonStop.setEnabled(False)

        # upgrade
        self.upgrade_thread = UpgradeThread(self.logger, "gui")
        self.upgrade_thread.progress_updated.connect(self.updateProgress)
        self.upgrade_thread.upgrade_finished.connect(self.upgradeFinished)
        self.upgrade_thread.start()

    def showVersion(self):
        self.logger.debug(f"Tyutool version: {TYUTOOL_VERSION}")
        msg = QMessageBox(self)
        msg.setWindowTitle("Tyutool Version")
        msg.setText(TYUTOOL_VERSION)
        msg.exec()


def show():
    _dbg("show() called, creating QApplication...")
    print("[MAIN] Creating QApplication...", flush=True)
    # Linux: global/D-Bus menu bar exports only normal menus; custom widgets vanish.
    if sys.platform.startswith("linux"):
        QtCore.QCoreApplication.setAttribute(
            QtCore.Qt.ApplicationAttribute.AA_DontUseNativeMenuBar,
            True,
        )
    app = QtWidgets.QApplication([])
    print("[MAIN] Creating MyWidget...", flush=True)
    _dbg("creating MyWidget...")
    widget = MyWidget()

    def _cleanup():
        print("[MAIN] aboutToQuit: cleaning up threads...", flush=True)
        _dbg("aboutToQuit: cleaning up threads...")
        widget._wait_threads()
        print("[MAIN] aboutToQuit: cleanup done", flush=True)

    app.aboutToQuit.connect(_cleanup)
    _dbg("widget.show()...")
    print("[MAIN] widget.show()...", flush=True)
    widget.show()
    _dbg("entering event loop (app.exec)...")
    print("[MAIN] Entering event loop...", flush=True)
    ret = app.exec()
    print(f"[MAIN] Event loop exited with ret={ret}", flush=True)
    _dbg(f"event loop exited with {ret}")
    sys.exit(ret)
