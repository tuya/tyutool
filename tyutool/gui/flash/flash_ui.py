#!/usr/bin/env python
# -*- coding: utf-8 -*-

import os
import sys
import json
import base64
from urllib.request import urlopen
from serial.tools import list_ports

from PySide6 import QtWidgets
from PySide6.QtCore import QRegularExpression, QThread, Signal
from PySide6.QtGui import QRegularExpressionValidator
from PySide6.QtGui import QPixmap
from PySide6.QtWidgets import QFileDialog

from ..ui_logo import LOGO_PNG_BYTES
from tyutool.flash import FlashArgv, ProgressHandler, FlashInterface
from tyutool.flash import flash_params_check
from tyutool.util.util import tyutool_root


class GuiProgressHandler(QThread, ProgressHandler):
    update_signal = Signal(int)

    def __init__(self, pg):
        super().__init__()
        self.pg = pg
        self.value = 0
        self.pg.reset()
        pass

    def setup(self, header, total):
        self.pg.setFormat(f'{header}: %p%')
        self.pg.setRange(0, total)
        pass

    def start(self):
        self.value = 0
        self.update_signal.emit(self.value)
        pass

    def update(self, size=1):
        self.value += size
        # self.pg.setValue(self.value)  # Accessing parent UI control here causes crash
        self.update_signal.emit(self.value)
        pass

    def close(self):
        self.value = 0
        self.pg.reset()


class FlashGUI(QtWidgets.QMainWindow):
    def __init__(self):
        super().__init__()
        pass

    def cacheSave(self):
        cache_data = {
            "operate": self.ui.comboBoxOperate.currentText(),
            "chip": self.ui.comboBoxChip.currentText(),
            "module": self.ui.comboBoxModule.currentText(),
            "file_in": self.file_in,
            "file_out": self.file_out,
            "baudrate": self.ui.comboBoxBaud.currentText(),
            "start": self.ui.lineEditStart.text(),
            "length": self.ui.lineEditLength.text(),
        }
        json_str = json.dumps(cache_data, indent=4, ensure_ascii=False)
        with open(self.gui_flash_cache, 'w', encoding='utf-8') as f:
            f.write(json_str)
        pass

    def cacheUse(self):
        cache_data = {}
        if os.path.exists(self.gui_flash_cache):
            f = open(self.gui_flash_cache, 'r', encoding='utf-8')
            cache_data = json.load(f)
            f.close()

        operate = cache_data.get('operate', "Write")
        self.file_in = cache_data.get('file_in', "")
        self.file_out = cache_data.get('file_out', "")
        if operate == "Read":
            self.ui.comboBoxOperate.setCurrentIndex(1)
            binfile = self.file_out
            self.ui.labelFile.setText("File Out")
            self.ui.pushButtonBrowse.clicked.connect(self.btnFileOutputClicked)
            self.ui.lineEditLength.setEnabled(True)
        else:  # Write
            self.ui.comboBoxOperate.setCurrentIndex(0)
            binfile = self.file_in
            self.ui.labelFile.setText("File In")
            self.ui.pushButtonBrowse.clicked.connect(self.btnFileInputClicked)
            self.ui.lineEditLength.setEnabled(False)
        self.ui.lineEditFile.setText(binfile)

        chip = cache_data.get('chip', "BK7231N")
        module = cache_data.get('module', "CBU")
        self.ui.comboBoxChip.setCurrentText(chip)
        self.modules = FlashInterface.get_modules(chip)
        self.ui.comboBoxModule.addItems(self.modules.keys())
        self.ui.comboBoxModule.setCurrentText(module)

        baudrate = cache_data.get('baudrate', "")
        start = cache_data.get('start', "")
        length = cache_data.get('length', "")
        if baudrate:
            self.ui.comboBoxBaud.setCurrentText(baudrate)
        if start:
            self.ui.lineEditStart.setText(start)
        if length:
            self.ui.lineEditLength.setText(length)

    def flashUiSetup(self):
        # Validation rules
        self.ui.comboBoxOperate.addItems(['Write', 'Read'])
        self.default_baudrates = ["115200", "230400", "460800",
                                  "921600", "1500000", "2000000"]
        reg_baud = QRegularExpression('[0-9]+$')
        reg_addr = QRegularExpression('^0x[0-9a-fA-F]+$')
        validator_baud = QRegularExpressionValidator(reg_baud)
        self.ui.comboBoxBaud.setValidator(validator_baud)
        validator_addr = QRegularExpressionValidator(reg_addr)
        self.ui.lineEditStart.setValidator(validator_addr)
        self.ui.lineEditLength.setValidator(validator_addr)
        self.ui.lineEditLength.setText("0x1000")
        # Default values
        self.gui_flash_cache = os.path.join(self.cache_dir, "gui_flash.cache")
        self.file_in = ""
        self.file_out = ""
        self.chips = list(FlashInterface.get_soc_names())
        self.ui.comboBoxChip.addItems(self.chips)
        self.modules = FlashInterface.get_modules(self.chips[0])
        baudrate = FlashInterface.get_baudrate(self.chips[0])
        baudrate_item = self.default_baudrates.copy()
        baudrate_item.insert(0, str(baudrate))
        self.ui.comboBoxBaud.addItems(baudrate_item)
        start_addr = FlashInterface.get_start_addr(self.chips[0])
        self.ui.lineEditStart.setText(f'{start_addr:#04x}')
        # self.ui.comboBoxModule.addItems(self.modules.keys())
        self.ui.labelModulePic.setScaledContents(True)
        self.pixmap = QPixmap()
        logo_png = base64.b64decode(LOGO_PNG_BYTES)
        self.pixmap.loadFromData(logo_png)
        self.ui.labelModulePic.setPixmap(self.pixmap)
        # self.ui.labelModulePic.setStyleSheet(
        #     "border-image:url(./resource/logo.png);")
        self.ui.textBrowserShow.setStyleSheet(
            "color: rgb(37, 214, 78); background-color: rgb(35, 35, 35);")
        self.ui.labelModuleUrl.setText(
            '<a style="color: black;" href=\"https://iot.tuya.com\">Tuya</a>')
        self.cacheUse()
        # Bindings and triggers
        self.progress = GuiProgressHandler(self.ui.progressBarShow)
        self.progress.update_signal.connect(self.updateProgressValue)
        self.ui.comboBoxOperate.currentTextChanged[str].connect(
            self.operateChanged)
        self.ui.comboBoxChip.currentTextChanged[str].connect(
            self.cmbChipTextChanged)
        self.ui.comboBoxModule.currentTextChanged[str].connect(
            self.cmbModuleTextChanged)
        self.ui.pushButtonRescan.clicked.connect(self.btnRescanClicked)
        self.ui.pushButtonStart.clicked.connect(self.btnStartClicked)
        self.ui.pushButtonStop.clicked.connect(self.btnStopClicked)

        self.flash_do = FlashDo()

        pass

    def updateProgressValue(self, value):
        self.ui.progressBarShow.setValue(value)
        pass

    def cmbChipTextChanged(self, chip):
        self.ui.comboBoxModule.clear()
        if not chip:
            return False
        self.modules = FlashInterface.get_modules(chip)
        self.ui.comboBoxModule.addItems(list(self.modules.keys()))

        baudrate = FlashInterface.get_baudrate(chip)
        baudrate_item = self.default_baudrates.copy()
        baudrate_item.insert(0, str(baudrate))
        self.ui.comboBoxBaud.clear()
        self.ui.comboBoxBaud.addItems(baudrate_item)
        start_addr = FlashInterface.get_start_addr(chip)
        self.ui.lineEditStart.setText(f'{start_addr:#04x}')
        pass

    def cmbModuleTextChanged(self, mod):
        if not mod:
            return False
        url = self.modules[mod]['url']
        pic = self.modules[mod]['pic']
        url_string = f'<a style="color: black;" href=\"{url}\">Details</a>'
        self.ui.labelModuleUrl.setText(url_string)
        self.logger.debug(f'url: {url}')
        self.logger.debug(f'pic: {pic}')

        # pic
        pic_context = b''
        try:
            pic_context = urlopen(pic, timeout=2).read()
        except Exception as e:
            # Do not return here; showing a blank image is acceptable
            self.logger.error(e)
            self.logger.error(f'Network connection failure [{pic}]')
        self.pixmap.loadFromData(pic_context)
        self.ui.labelModulePic.setPixmap(self.pixmap)
        pass

    def btnFileInputClicked(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        default_dir = tyutool_root()
        if self.file_in:
            default_dir = os.path.dirname(self.file_in)
        file_path, _ = QFileDialog.getOpenFileName(
            self,
            "选择烧录文件",
            default_dir,
            "文件类型 (*.bin)"
        )
        self.logger.debug(f'input file: {file_path}')
        if file_path:
            self.file_in = file_path
            self.ui.lineEditFile.setText(file_path)
        pass

    def btnFileOutputClicked(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        default_path = os.path.join(tyutool_root(), "read.bin")
        if self.file_out:
            default_path = self.file_out
        file_path, _ = QFileDialog.getSaveFileName(
            self,
            "Output",
            default_path,
            "BIN (*.bin)"
        )

        self.logger.debug(f'output file: {file_path}')
        if file_path:
            self.file_out = file_path
            self.ui.lineEditFile.setText(file_path)
        pass

    def operateChanged(self, t):
        self.logger.debug(sys._getframe().f_code.co_name)
        self.ui.lineEditFile.clear()
        self.ui.pushButtonBrowse.clicked.disconnect()
        if t == "Read":
            binfile = self.file_out
            self.ui.labelFile.setText("File Out")
            self.ui.pushButtonBrowse.clicked.connect(self.btnFileOutputClicked)
            self.ui.lineEditLength.setEnabled(True)
        else:  # Write
            binfile = self.file_in
            self.ui.labelFile.setText("File In")
            self.ui.pushButtonBrowse.clicked.connect(self.btnFileInputClicked)
            self.ui.lineEditLength.setEnabled(False)
        self.ui.lineEditFile.setText(binfile)
        pass

    def _log_usbx_devices(self):
        """Enumerate USB devices via usbx and log details (cross-platform, no libusb required)."""
        try:
            from usbx import usb
        except ImportError:
            self.logger.debug("usbx 未安装，跳过 USB 设备枚举。可安装: pip install usbx")
            return
        try:
            devices = list(usb.get_devices())
            if not devices:
                self.logger.debug("usbx: 未发现 USB 设备")
                return
            self.logger.debug(f"usbx: 共发现 {len(devices)} 个 USB 设备")
            self.logger.debug(f"====== usbx devices start ====== ")
            for dev in devices:
                vid = getattr(dev, 'vid', None)
                pid = getattr(dev, 'pid', None)
                bcd = getattr(dev, 'device_version', None)
                bcd_str = f"{bcd}" if bcd is not None else "n/a"
                self.logger.debug(
                    f"  USB device id={getattr(dev, 'identifier', '')} "
                    f"VID:PID={(vid or 0):04X}:{(pid or 0):04X} device_version={bcd_str}"
                )
                manufacturer = getattr(dev, 'manufacturer', None) or ""
                product = getattr(dev, 'product', None) or ""
                serial = getattr(dev, 'serial', None) or ""
                if manufacturer or product or serial:
                    self.logger.debug(
                        f"    manufacturer={manufacturer} product={product} serial={serial}"
                    )
            self.logger.debug(f"====== usbx devices end ====== ")
        except Exception as e:
            self.logger.debug(f"usbx: 枚举 USB 设备失败: {e}")

    def btnRescanClicked(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        self._log_usbx_devices()
        port_items = []
        self.ui.comboBoxPort.clear()
        port_list = list(list_ports.comports())
        if len(port_list) > 0:
            for port in port_list:
                device = port.device if hasattr(port, 'device') else port[0]
                if device.startswith("/dev/ttyS"):
                    continue
                # Log extra USB info from ListPortInfo (cross-platform)
                self.logger.debug(
                    f'port: {device} | desc: {getattr(port, "description", "n/a")} | '
                    f'hwid: {getattr(port, "hwid", "n/a")}'
                )
                vid = getattr(port, 'vid', None)
                pid = getattr(port, 'pid', None)
                if vid is not None or pid is not None:
                    self.logger.debug(
                        f'  USB VID:PID={(vid or 0):04X}:{(pid or 0):04X} '
                        f'serial={getattr(port, "serial_number", "") or ""} '
                        f'manufacturer={getattr(port, "manufacturer", "") or ""} '
                        f'product={getattr(port, "product", "") or ""}'
                    )
                port_items.append(device)
        port_items.sort()
        self.ui.comboBoxPort.addItems(port_items)
        pass

    def btnStartClicked(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        chip = self.ui.comboBoxChip.currentText()
        operate = self.ui.comboBoxOperate.currentText()
        binfile = self.ui.lineEditFile.text()
        port = self.ui.comboBoxPort.currentText()
        baud = self.ui.comboBoxBaud.currentText()
        start = self.ui.lineEditStart.text()
        length = self.ui.lineEditLength.text()
        self.logger.debug(f'start: {start}')
        self.logger.debug(f'length: {length}')

        baudrate = int(baud)
        start_addr = 0x00
        read_length = 0x1000
        if start:
            start_addr = int(start, 16)
        if length:
            read_length = int(length, 16)

        self.cacheSave()
        # check params
        argv = FlashArgv(operate, chip, port, baudrate, start_addr, binfile,
                         length=read_length)
        if not flash_params_check(argv, logger=self.logger):
            self.logger.error("Parameter check failure.")
            return False

        handler_obj = FlashInterface.get_flash_handler(chip)
        soc_handler = handler_obj(argv,
                                  logger=self.logger,
                                  progress=self.progress)
        self.flash_do.config(soc_handler, operate, read_length,
                             self.ui.pushButtonStart)
        self.flash_do.start()  # Start worker thread
        self.ui.pushButtonStart.setEnabled(False)

        self.logger.info(f'{operate} Start.')
        return True

    def btnStopClicked(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        if self.flash_do.isRunning():
            self.logger.debug("flash runing ...")
            self.flash_do.stop()
            self.flash_do.quit()
            self.flash_do.wait()
            self.logger.info("Stoped.")
        pass


class FlashDo(QThread):
    def __init__(self):
        super(FlashDo, self).__init__()
        pass

    def config(self, soc_handler, operate, read_length, btn):
        self.soc_handler = soc_handler
        self.operate = operate
        self.read_length = read_length
        self.btn = btn
        self.soc_handler.start()
        pass

    def run(self):
        operate = self.operate
        soc_handler = self.soc_handler
        read_length = self.read_length
        if operate == "Write":
            if soc_handler.shake() \
                    and soc_handler.erase() \
                    and soc_handler.write():
                soc_handler.crc_check()
        elif operate == "Read":
            if soc_handler.shake() \
                    and soc_handler.read(read_length):
                soc_handler.crc_check()
        soc_handler.reboot()
        soc_handler.serial_close()
        self.btn.setEnabled(True)

    def stop(self):
        self.soc_handler.stop()
        pass
