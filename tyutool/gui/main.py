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
from PySide6.QtCore import QEventLoop, QTimer
from PySide6.QtWidgets import QApplication, QMessageBox

from .ui_main import Ui_MainWindow
from .ui_logo import LOGO_ICON_BYTES

from tyutool.gui.flash import FlashGUI
from tyutool.gui.serial import SerialGUI
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


#  class QTextEditHandler(logging.Handler):
#      def __init__(self, text_edit):
#          super().__init__()
#          self.text_edit = text_edit
#
#      def emit(self, record):
#          msg = self.format(record)
#          self.text_edit.append(msg)


class MyWidget(FlashGUI, SerialGUI):
    def __init__(self):
        super().__init__()
        # 下面将输出重定向到textBrowser中(屏蔽下面两句恢复终端日志)
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

        #  handler = QTextEditHandler(ui.textBrowserShow)
        #  self.logger = set_logger(logging.INFO, handler=handler)
        self.logger = set_logger(logging.INFO)
        self.logger.info("Show Tuya Uart Tool.")

        self.flashUiSetup()
        self.serialUiSetup()

        self.ui.menuDebug.triggered[QAction].connect(self.LogDebugSwitch)
        self.ui.actionUpgrade.triggered.connect(self.guiUpgrade)
        self.ui.actionVersion.triggered.connect(self.showVersion)

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
        up_handle = TyutoolUpgrade(self.logger, "gui")
        up_handle.upgrade()
        pass

    def guiAskUpgrade(self):
        def ask(server_version=""):
            msg = QMessageBox(self)
            msg.setWindowTitle("Upgrade")
            msg.setText(f"Upgrade Tyutool to [{server_version}] ?")
            # 添加自定义按钮
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
        up_handle.ask_upgrade(ask)
        pass

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
