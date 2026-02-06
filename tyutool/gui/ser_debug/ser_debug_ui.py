#!/usr/bin/env python
# -*- coding: utf-8 -*-

import logging
from serial.tools import list_ports

from PySide6 import QtWidgets
from PySide6.QtCore import QRegularExpression, QObject, Signal
from PySide6.QtGui import QRegularExpressionValidator

from tyutool.ai_debug import SerAIDebugMonitor


def _check_sd_monitor(func):
    def wrapper(self, *args, **kwargs):
        if not self.sd_monitor:
            self.sd_logger.warning("sd_monitor not initialized.")
            return
        return func(self, *args, **kwargs)
    return wrapper


class SDLogSignal(QObject):
    log_message = Signal(str)
    pass


class SDQTextEditHandler(logging.Handler):
    def __init__(self, text_edit):
        super().__init__()
        self.text_edit = text_edit
        self.log_signal = SDLogSignal()
        self.log_signal.log_message.connect(self._append_log)
        pass

    def emit(self, record):
        msg = self.format(record)
        self.log_signal.log_message.emit(msg)
        pass

    def _append_log(self, msg):
        self.text_edit.append(msg)
        pass


class SerDebugGUI(QtWidgets.QMainWindow):
    ALG_PARAMS = {
        "aec_ec_depth": "20",
        "aec_init_flags": "1",
        "aec_mic_delay": "0",
        "aec_ref_scale": "0",
        "aec_voice_vol": "13",
        "aec_TxRxThr": "30",
        "aec_TxRxFlr": "6",
        "aec_ns_level": "5",
        "aec_ns_para": "2",
        "aec_drc": "4",
    }

    def __init__(self):
        super().__init__()
        self.sd_monitor = None
        self.sd_logger = None
        pass

    def _setupLogger(self):
        logger = logging.getLogger("tyut_ser_debug")
        logger.setLevel(logging.DEBUG)
        log_format = "[%(levelname)s]: %(message)s"
        lf = logging.Formatter(fmt=log_format)
        handler = SDQTextEditHandler(self.ui.textBrowserSD)
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
        self.ui.comboBoxSDBaud.setCurrentText("460800")

        self.ui.comboBoxSDAlgSetP.addItems(self.ALG_PARAMS)
        self.ui.lineEditSDAlgSetV.setPlaceholderText("1")
        self.ui.comboBoxSDAlgGetP.addItems(self.ALG_PARAMS)

        self.ui.textBrowserSD.setStyleSheet(
            "color: rgb(37, 214, 78); background-color: rgb(35, 35, 35);")

        self.ui.pushButtonSDCom.clicked.connect(
            self.pushButtonSDComClicked
        )
        self.ui.pushButtonSDConnect.clicked.connect(
            self.pushButtonSDConnectClicked
        )

        self.ui.pushButtonSDStart.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("start")
        )
        self.ui.pushButtonSDStop.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("stop")
        )
        self.ui.pushButtonSDReset.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("reset")
        )
        self.ui.pushButtonSDDump0.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("dump 0")
        )
        self.ui.pushButtonSDDump1.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("dump 1")
        )
        self.ui.pushButtonSDDump2.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("dump 2")
        )
        self.ui.pushButtonSDDump3.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("dump 3")
        )
        self.ui.pushButtonSDDump4.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("dump 4")
        )
        self.ui.pushButtonSDBg0.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("bg 0")
        )
        self.ui.pushButtonSDBg1.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("bg 1")
        )
        self.ui.pushButtonSDBg2.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("bg 2")
        )
        self.ui.pushButtonSDBg3.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("bg 3")
        )
        self.ui.pushButtonSDBg4.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("bg 4")
        )

        self.ui.pushButtonSDPinSet.clicked.connect(
            self.pushButtonSDPinSetClicked
        )
        self.ui.pushButtonSDVolume.clicked.connect(
            self.pushButtonSDVolumeClicked
        )
        self.ui.pushButtonSDMicgain.clicked.connect(
            self.pushButtonSDMicgainClicked
        )
        self.ui.pushButtonSDAlgSet.clicked.connect(
            self.pushButtonSDAlgSetClicked
        )
        self.ui.pushButtonSDAlgSetVad.clicked.connect(
            self.pushButtonSDAlgSetVadClicked
        )
        self.ui.pushButtonSDAlgGet.clicked.connect(
            self.pushButtonSDAlgGetClicked
        )
        self.ui.pushButtonSDAlgDump.clicked.connect(
            lambda: self.pushButtonSDCmdClicked("alg dump")
        )
        self.ui.pushButtonAutoTest.clicked.connect(
            self.pushButtonAutoTestClicked
        )

        self.ui.comboBoxSDAlgSetP.currentIndexChanged.connect(
            self.comboBoxSDAlgSetPChanged
        )

        self._setupLogger()
        pass

    def pushButtonSDComClicked(self):
        port_list = list(list_ports.comports())
        port_items = []
        self.ui.comboBoxSDPort.clear()
        if len(port_list) > 0:
            for port in port_list:
                pl = list(port)
                if pl[0].startswith("/dev/ttyS"):
                    continue
                port_items.append(pl[0])
        port_items.sort()
        self.ui.comboBoxSDPort.addItems(port_items)
        pass

    def pushButtonSDConnectClicked(self):
        port = self.ui.comboBoxSDPort.currentText()
        baudrate = int(self.ui.comboBoxSDBaud.currentText())
        sd_monitor = SerAIDebugMonitor(
            port, baudrate, "ser_ai_debug", self.sd_logger, gui_mode=True
        )

        if not sd_monitor.open_port():
            self.sd_logger.error("Open port failed.")
            self.sd_monitor = None
            return

        sd_monitor.start_reading()
        self.sd_monitor = sd_monitor
        self.ui.pushButtonSDConnect.setText("Quit")
        self.ui.pushButtonSDConnect.clicked.disconnect()
        self.ui.pushButtonSDConnect.clicked.connect(
            self.pushButtonSDDisconnectClicked
        )
        self.sd_logger.info("Open port success.")
        pass

    def pushButtonSDDisconnectClicked(self):
        self.sd_monitor.stop_reading()
        self.sd_monitor.close_port()
        self.sd_monitor = None

        self.ui.pushButtonSDConnect.setText("Connect")
        self.ui.pushButtonSDConnect.clicked.disconnect()
        self.ui.pushButtonSDConnect.clicked.connect(
            self.pushButtonSDConnectClicked
        )
        self.sd_logger.info("Quit success.")
        pass

    @_check_sd_monitor
    def pushButtonSDCmdClicked(self, cmd=""):
        self.sd_monitor.process_input_cmd(cmd)
        pass

    @_check_sd_monitor
    def pushButtonSDPinSetClicked(self):
        typ = self.ui.lineEditSDPinSetT.text()
        value = self.ui.lineEditSDPinSetV.text()
        self.sd_monitor.process_input_cmd(f"pin_set {typ} {value}")
        pass

    @_check_sd_monitor
    def pushButtonSDVolumeClicked(self):
        volume = self.ui.lineEditSDVolume.text()
        self.sd_monitor.process_input_cmd(f"volume {volume}")
        pass

    @_check_sd_monitor
    def pushButtonSDMicgainClicked(self):
        micgain = self.ui.lineEditSDMicgain.text()
        self.sd_monitor.process_input_cmd(f"micgain {micgain}")
        pass

    @_check_sd_monitor
    def pushButtonSDAlgSetClicked(self):
        param = self.ui.comboBoxSDAlgSetP.currentText()
        value = self.ui.lineEditSDAlgSetV.text()
        self.sd_monitor.process_input_cmd(f"alg set {param} {value}")
        pass

    @_check_sd_monitor
    def pushButtonSDAlgSetVadClicked(self):
        channel = self.ui.lineEditSDAlgSetVadC.text()
        value = self.ui.lineEditSDAlgSetVadV.text()
        self.sd_monitor.process_input_cmd(f"alg set vad_SPthr {channel} {value}")
        pass

    @_check_sd_monitor
    def pushButtonSDAlgGetClicked(self):
        param = self.ui.comboBoxSDAlgGetP.currentText()
        self.sd_monitor.process_input_cmd(f"alg get {param}")
        pass

    @_check_sd_monitor
    def pushButtonAutoTestClicked(self):
        self.sd_monitor.auto_test()
        pass

    def comboBoxSDAlgSetPChanged(self, idx):
        param = self.ui.comboBoxSDAlgSetP.currentText()
        holder = self.ALG_PARAMS.get(param, "")
        self.ui.lineEditSDAlgSetV.setPlaceholderText(holder)
        pass
