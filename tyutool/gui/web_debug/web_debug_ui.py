#!/usr/bin/env python
# -*- coding: utf-8 -*-

import logging

from PySide6 import QtWidgets
from PySide6.QtCore import QRegularExpression, QObject, Signal
from PySide6.QtGui import QRegularExpressionValidator

from tyutool.ai_debug import WebAIDebugMonitor


class webLogSignal(QObject):
    log_message = Signal(str)
    pass


class webQTextEditHandler(logging.Handler):
    def __init__(self, text_edit):
        super().__init__()
        self.text_edit = text_edit
        self.log_signal = webLogSignal()
        self.log_signal.log_message.connect(self._append_log)
        pass

    def emit(self, record):
        msg = self.format(record)
        self.log_signal.log_message.emit(msg)
        pass

    def _append_log(self, msg):
        self.text_edit.append(msg)
        pass


class WebDebugGUI(QtWidgets.QMainWindow):
    def __init__(self):
        super().__init__()
        self.wd_monitor = None
        self.wd_logger = None
        pass

    def _setupWebLogger(self):
        logger = logging.getLogger("tyut_web_debug")
        logger.setLevel(logging.INFO)
        log_format = "[%(levelname)s]: %(message)s"
        lf = logging.Formatter(fmt=log_format)
        handler = webQTextEditHandler(self.ui.textBrowserWD)
        handler.setFormatter(lf)
        logger.addHandler(handler)
        logger.info("tyut_web_debug init done.")
        self.wd_logger = logger
        pass

    def webDebugUiSetup(self):
        self.ui.textBrowserWD.setStyleSheet(
            "color: rgb(37, 214, 78); background-color: rgb(35, 35, 35);")

        reg_port = QRegularExpression('[0-9]+$')
        validator_port = QRegularExpressionValidator(reg_port)
        self.ui.lineEditWDPort.setValidator(validator_port)

        self.ui.pushButtonWDConnect.clicked.connect(
            self.pushButtonWDConnectClicked
        )
        self.ui.pushButtonWDClear.clicked.connect(
            self.pushButtonWDClearClicked
        )
        self.ui.checkBoxWDDebug.stateChanged.connect(
            self.checkBoxWDDebugChecked
        )

        self._setupWebLogger()
        pass

    def pushButtonWDConnectClicked(self):
        event = []
        if self.ui.checkBoxWDText.isChecked():
            event.append('t')
        if self.ui.checkBoxWDAudio.isChecked():
            event.append('a')
        if self.ui.checkBoxWDPicture.isChecked():
            event.append('p')
        if self.ui.checkBoxWDVideo.isChecked():
            event.append('v')

        ip = self.ui.lineEditWDIP.text()
        port = int(self.ui.lineEditWDPort.text())
        save = self.ui.lineEditWDSave.text()

        monitor = WebAIDebugMonitor(ip, port, event, save, self.wd_logger)

        # Connect to server
        if not monitor.connect():
            self.wd_logger.error("Open socket success.")
            return

        self.wd_monitor = monitor

        self.ui.pushButtonWDConnect.setText("Quit")
        self.ui.pushButtonWDConnect.clicked.disconnect()
        self.ui.pushButtonWDConnect.clicked.connect(
            self.pushButtonWDDisconnectClicked
        )
        self.wd_logger.info("Open socket success.")
        pass

    def pushButtonWDDisconnectClicked(self):
        self.wd_monitor.disconnect()
        self.wd_monitor = None

        self.ui.pushButtonWDConnect.setText("Connect")
        self.ui.pushButtonWDConnect.clicked.disconnect()
        self.ui.pushButtonWDConnect.clicked.connect(
            self.pushButtonWDConnectClicked
        )
        self.wd_logger.info("Quit success.")
        pass

    def pushButtonWDClearClicked(self):
        self.ui.textBrowserWD.clear()
        pass

    def checkBoxWDDebugChecked(self, state):
        if self.ui.checkBoxWDDebug.isChecked():
            self.wd_logger.setLevel(logging.DEBUG)
            self.wd_logger.debug("Debug on.")
        else:
            self.wd_logger.setLevel(logging.INFO)
            self.wd_logger.info("Debug off.")
        pass
