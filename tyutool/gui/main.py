#!/usr/bin/env python
# -*- coding: utf-8 -*-

import os
import sys
import base64
import logging
import logging.handlers

from PySide6 import QtCore, QtWidgets, QtGui
# from PySide6.QtWidgets import QFileDialog
from PySide6.QtGui import QAction, QIcon, QPixmap
from PySide6.QtCore import QEventLoop, QTimer, QThread, Signal
from PySide6.QtWidgets import QApplication, QMessageBox

from .ui_main import Ui_MainWindow
from .ui_logo import LOGO_ICON_BYTES

from tyutool.gui.flash import FlashGUI
from tyutool.gui.serial import SerialGUI
from tyutool.gui.ser_debug import SerDebugGUI
from tyutool.util import TyutoolUpgrade
from tyutool.util.util import tyutool_root, set_logger, TYUTOOL_VERSION


class EmittingStr(QtCore.QObject):
    textWritten = QtCore.Signal(str)

    def write(self, text):
        self.textWritten.emit(str(text))
        loop = QEventLoop()
        QTimer.singleShot(100, loop.quit)
        loop.exec_()
        QApplication.processEvents()

    def flush(self):
        pass


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


class MyWidget(FlashGUI, SerialGUI, SerDebugGUI):
    def __init__(self):
        super().__init__()

        # log to textBrowser or to terminal
        sys.stderr = EmittingStr()
        sys.stderr.textWritten.connect(self.outputWritten)

        ui = Ui_MainWindow()
        ui.setupUi(self)
        self.ui = ui

        # cache
        self.cache_dir = os.path.join(tyutool_root(), "cache")
        if not os.path.exists(self.cache_dir):
            os.mkdir(self.cache_dir)

        # ico
        logo_icon = base64.b64decode(LOGO_ICON_BYTES)
        icon_pixmap = QPixmap()
        icon_pixmap.loadFromData(logo_icon)
        self.setWindowIcon(QIcon(icon_pixmap))

        self.logger = set_logger(logging.INFO)
        self.logger.info("Show Tuya Uart Tool.")

        self.flashUiSetup()
        self.serialUiSetup()
        self.serDebugUiSetup()

        self.ui.menuDebug.triggered[QAction].connect(self.LogDebugSwitch)
        self.ui.actionUpgrade.triggered.connect(self.guiUpgrade)
        self.ui.actionVersion.triggered.connect(self.showVersion)

        self.upgrade_thread = None

        QTimer.singleShot(500, self.guiAskUpgrade)

        pass

    def outputWritten(self, text):
        cursor = self.ui.textBrowserShow.textCursor()
        cursor.movePosition(QtGui.QTextCursor.End)
        cursor.insertText(text)
        self.ui.textBrowserShow.setTextCursor(cursor)
        self.ui.textBrowserShow.ensureCursorVisible()
        pass

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
        def ask(server_version=""):
            msg = QMessageBox(self)
            msg.setWindowTitle("Upgrade")
            msg.setText(f"Upgrade Tyutool to [{server_version}] ?")

            confirm_button = msg.addButton("Ok", QMessageBox.ActionRole)
            skip_button = msg.addButton("Skip", QMessageBox.ActionRole)
            cancel_button = msg.addButton("Cancel", QMessageBox.RejectRole)
            msg.exec()
            if msg.clickedButton() == confirm_button:
                result = 0
            elif msg.clickedButton() == cancel_button:
                result = 1
            elif msg.clickedButton() == skip_button:
                result = 2
            return result

        up_handle = TyutoolUpgrade(self.logger, "gui")
        should_upgrade = up_handle.ask_upgrade(ask, auto_upgrade=False)

        if should_upgrade:
            QTimer.singleShot(100, self.startUpgradeWithProgress)
        pass

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
    app = QtWidgets.QApplication([])
    widget = MyWidget()
    widget.show()
    sys.exit(app.exec())
