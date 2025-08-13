#!/usr/bin/env python
# -*- coding: utf-8 -*-

import os
import sys
import serial
import logging
from serial.tools import list_ports

from PySide6 import QtWidgets
from PySide6.QtWidgets import (
    QLineEdit, QHBoxLayout, QVBoxLayout, QDialogButtonBox, QPushButton)
from PySide6.QtCore import QRegularExpression
from PySide6.QtGui import QRegularExpressionValidator

from tyutool.ai_debug import SerAIDebugMonitor


class QTextEditHandler(logging.Handler):
    def __init__(self, text_edit):
        super().__init__()
        self.text_edit = text_edit

    def emit(self, record):
        msg = self.format(record)
        self.text_edit.append(msg)


class SerDebugGUI(QtWidgets.QMainWindow):
    def __init__(self):
        super().__init__()
        self.monitor = None
        pass

    def _setupLogger(self):
        logger = logging.getLogger("tyut_ser_debug")
        logger.setLevel(logging.DEBUG)
        log_format = "[%(levelname)s]: %(message)s"
        lf = logging.Formatter(fmt=log_format)
        handler = QTextEditHandler(self.ui.textBrowserSD)
        handler.setFormatter(lf)
        logger.addHandler(handler)
        logger.info("tyut_ser_debug init done.")
        self.sd_logger = logger
        pass

    def serDebugUiSetup(self):
        self.ui.comboBoxSDBaud.addItems(["115200", "230400",
                                        "460800", "921600"])
        reg_baud = QRegularExpression('[0-9]+$')
        validator_baud = QRegularExpressionValidator(reg_baud)
        self.ui.comboBoxSDBaud.setValidator(validator_baud)
        self.ui.textBrowserSD.setStyleSheet(
            "color: rgb(37, 214, 78); background-color: rgb(35, 35, 35);")

        self.ui.pushButtonSDCom.clicked.connect(self.pushButtonSDComClicked)
        self.ui.pushButtonSDConnect.clicked.connect(self.pushButtonSDConnectClicked)

        self.ui.pushButtonSDStart.clicked.connect(lambda: self.pushButtonSDCmdClicked("start"))
        self.ui.pushButtonSDStop.clicked.connect(lambda: self.pushButtonSDCmdClicked("stop"))
        self.ui.pushButtonSDReset.clicked.connect(lambda: self.pushButtonSDCmdClicked("reset"))
        self.ui.pushButtonSDDump0.clicked.connect(lambda: self.pushButtonSDCmdClicked("dump 0"))
        self.ui.pushButtonSDDump1.clicked.connect(lambda: self.pushButtonSDCmdClicked("dump 1"))
        self.ui.pushButtonSDDump2.clicked.connect(lambda: self.pushButtonSDCmdClicked("dump 2"))
        self.ui.pushButtonSDBg0.clicked.connect(lambda: self.pushButtonSDCmdClicked("bg 0"))
        self.ui.pushButtonSDBg1.clicked.connect(lambda: self.pushButtonSDCmdClicked("bg 1"))
        self.ui.pushButtonSDBg2.clicked.connect(lambda: self.pushButtonSDCmdClicked("bg 2"))
        self.ui.pushButtonSDBg3.clicked.connect(lambda: self.pushButtonSDCmdClicked("bg 3"))
        self.ui.pushButtonSDBg4.clicked.connect(lambda: self.pushButtonSDCmdClicked("bg 4"))

        self.ui.pushButtonSDVolume.clicked.connect(self.pushButtonSDVolumeClicked)
        self.ui.pushButtonSDMicgain.clicked.connect(self.pushButtonSDMicgainClicked)
        self.ui.pushButtonSDAlgSet.clicked.connect(self.pushButtonSDAlgSetClicked)
        self.ui.pushButtonSDAlgSetVad.clicked.connect(self.pushButtonSDAlgSetVadClicked)
        self.ui.pushButtonSDAlgGet.clicked.connect(self.pushButtonSDAlgGetClicked)
        self.ui.pushButtonSDAlgDump.clicked.connect(lambda: self.pushButtonSDCmdClicked("alg dump"))
        self._setupLogger()
        pass

    def pushButtonSDComClicked(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        port_list = list(list_ports.comports())
        port_items = []
        self.ui.comboBoxSDPort.clear()
        if len(port_list) > 0:
            for port in port_list:
                pl = list(port)
                if pl[0].startswith("/dev/ttyS"):
                    continue
                self.logger.debug(f'port info: {pl}')
                port_items.append(pl[0])
        port_items.sort()
        self.ui.comboBoxSDPort.addItems(port_items)
        pass

    def pushButtonSDConnectClicked(self):
        port = self.ui.comboBoxSDPort.currentText()
        baudrate = int(self.ui.comboBoxSDBaud.currentText())
        monitor = SerAIDebugMonitor(port, baudrate, "ser_ai_debug", self.sd_logger)

        if not monitor.open_port():
            self.sd_logger.error("Open port failed.")
            self.monitor = None
            return

        monitor.start_reading()
        self.monitor = monitor
        self.ui.pushButtonSDConnect.setText("Quit")
        self.ui.pushButtonSDConnect.clicked.disconnect()
        self.ui.pushButtonSDConnect.clicked.connect(self.pushButtonSDDisconnectClicked)
        self.sd_logger.info("Open port success.")
        pass

    def pushButtonSDDisconnectClicked(self):
        self.monitor.stop_reading()
        self.monitor.close_port()
        self.monitor = None

        self.ui.pushButtonSDConnect.setText("Start")
        self.ui.pushButtonSDConnect.clicked.disconnect()
        self.ui.pushButtonSDConnect.clicked.connect(self.pushButtonSDConnectClicked)
        self.sd_logger.info("Quit success.")
        pass

    def pushButtonSDCmdClicked(self, cmd=""):
        if not self.monitor:
            self.sd_logger.debug("Monitor not initialized.")
            return
        
        self.monitor.process_input_cmd(cmd)
        pass

    def pushButtonSDVolumeClicked(self):
        if not self.monitor:
            self.sd_logger.debug("Monitor not initialized.")
            return
        
        volume = self.ui.lineEditSDVolume.text()
        self.monitor.process_input_cmd(f"volume {volume}")
        pass

    def pushButtonSDMicgainClicked(self):
        if not self.monitor:
            self.sd_logger.debug("Monitor not initialized.")
            return
        
        micgain = self.ui.lineEditSDMicgain.currentText()
        self.monitor.process_input_cmd(f"micgain {micgain}")
        pass

    def pushButtonSDAlgSetClicked(self):
        if not self.monitor:
            self.sd_logger.debug("Monitor not initialized.")
            return
        
        param = self.ui.lineEditSDAlgSetP.text()
        value = self.ui.lineEditSDAlgSetV.text()
        self.monitor.process_input_cmd(f"alg set {param} {value}")
        pass

    def pushButtonSDAlgSetVadClicked(self):
        if not self.monitor:
            self.sd_logger.debug("Monitor not initialized.")
            return
        
        channel = self.ui.lineEditSDAlgSetVadC.text()
        value = self.ui.lineEditSDAlgSetVadV.text()
        self.monitor.process_input_cmd(f"alg set vad_SPthr {channel} {value}")
        pass

    def pushButtonSDAlgGetClicked(self):
        if not self.monitor:
            self.sd_logger.debug("Monitor not initialized.")
            return
        
        param = self.ui.lineEditSDAlgGetP.text()
        self.monitor.process_input_cmd(f"alg get {param}")
        pass