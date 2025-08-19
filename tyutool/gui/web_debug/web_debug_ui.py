#!/usr/bin/env python
# -*- coding: utf-8 -*-

import logging

from PySide6 import QtWidgets
from PySide6.QtCore import (
    Qt, QRegularExpression, QObject, Signal, QTimer
)
from PySide6.QtGui import QRegularExpressionValidator
from PySide6.QtWidgets import (
    QTextEdit
)

from tyutool.ai_debug import WebAIDebugMonitor


class WebDisplayHookSignal(QObject):
    hook_signal = Signal(object, str)
    pass


class WebGuiDataDisplayHook:
    def __init__(self, ui, logger):
        self.ui = ui
        self.logger = logger
        self.hook_signal = WebDisplayHookSignal()
        self.hook_signal.hook_signal.connect(self.show_packet)
        pass

    def hook(self, packet, msg):
        self.hook_signal.hook_signal.emit(packet, msg)
        pass

    def _scroll_to_bottom(self, scroll_area):
        scroll_bar = scroll_area.verticalScrollBar()
        scroll_bar.setValue(scroll_bar.maximum())
        QTimer.singleShot(0, lambda: scroll_bar.setValue(scroll_bar.maximum()))
        pass

    def _add_mag_to_scroll(self,
                           scroll_area, scroll_content, scroll_layout,
                           packet, msg):
        scroll_bar = scroll_area.verticalScrollBar()
        scroll_at_bottom = scroll_bar.value() >= (scroll_bar.maximum() - 1)

        text_edit = QTextEdit()
        text_edit.setText(msg)
        text_edit.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        text_edit.setReadOnly(True)
        text_edit.setLineWrapMode(QTextEdit.WidgetWidth)

        scroll_layout.addWidget(text_edit)
        text_edit.document().setTextWidth(text_edit.width())
        document_height = text_edit.document().size().height()
        text_edit.setMinimumHeight(int(document_height) + 2)
        text_edit.setMaximumHeight(int(document_height) + 2)
        scroll_content.updateGeometry()
        scroll_area.updateGeometry()

        if scroll_at_bottom:
            QTimer.singleShot(10, lambda: self._scroll_to_bottom(scroll_area))
        pass

    def show_packet(self, packet, msg):
        packet_type = packet['type']
        if packet_type == 31:  # Audio
            scroll_area = self.ui.scrollAreaWDAudio
            scroll_content = self.ui.scrollAreaWidgetContentsWDAudio
            scroll_layout = self.ui.verticalLayout_23
        elif packet_type == 34:  # Text
            scroll_area = self.ui.scrollAreaWDText
            scroll_content = self.ui.scrollAreaWidgetContentsWDText
            scroll_layout = self.ui.verticalLayout_22
        elif packet_type == 30:  # Video
            scroll_area = self.ui.scrollAreaWDVideo
            scroll_content = self.ui.scrollAreaWidgetContentsWDVideo
            scroll_layout = self.ui.verticalLayout_25
        elif packet_type == 32:  # Image
            scroll_area = self.ui.scrollAreaWDPicture
            scroll_content = self.ui.scrollAreaWidgetContentsWDPicture
            scroll_layout = self.ui.verticalLayout_24

        self._add_mag_to_scroll(
            scroll_area, scroll_content, scroll_layout, packet, msg
        )
        pass


class WebLogSignal(QObject):
    log_message = Signal(str)
    pass


class WebQTextEditHandler(logging.Handler):
    def __init__(self, text_edit):
        super().__init__()
        self.text_edit = text_edit
        self.log_signal = WebLogSignal()
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
        handler = WebQTextEditHandler(self.ui.textBrowserWD)
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
        display_hook = WebGuiDataDisplayHook(self.ui, self.wd_logger)

        monitor = WebAIDebugMonitor(
            ip, port, event, save, self.wd_logger,
            display_hook
        )

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

    def _clear_scroll_area(self, scroll_area, scroll_content, scroll_layout):
        while scroll_layout.count():
            item = scroll_layout.takeAt(0)
            widget = item.widget()
            if widget:
                widget.deleteLater()
        scroll_content.updateGeometry()
        scroll_area.updateGeometry()
        pass

    def pushButtonWDClearClicked(self):
        self.ui.textBrowserWD.clear()
        self._clear_scroll_area(
            self.ui.scrollAreaWDText,
            self.ui.scrollAreaWidgetContentsWDText,
            self.ui.verticalLayout_22)
        self._clear_scroll_area(
            self.ui.scrollAreaWDAudio,
            self.ui.scrollAreaWidgetContentsWDAudio,
            self.ui.verticalLayout_23)
        self._clear_scroll_area(
            self.ui.scrollAreaWDPicture,
            self.ui.scrollAreaWidgetContentsWDPicture,
            self.ui.verticalLayout_24)
        self._clear_scroll_area(
            self.ui.scrollAreaWDVideo,
            self.ui.scrollAreaWidgetContentsWDVideo,
            self.ui.verticalLayout_25)
        pass

    def checkBoxWDDebugChecked(self, state):
        if self.ui.checkBoxWDDebug.isChecked():
            self.wd_logger.setLevel(logging.DEBUG)
            self.wd_logger.debug("Debug on.")
        else:
            self.wd_logger.setLevel(logging.INFO)
            self.wd_logger.info("Debug off.")
        pass
