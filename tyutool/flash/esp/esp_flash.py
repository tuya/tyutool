#!/usr/bin/env python
# -*- coding: utf-8 -*-

import os
import sys
import logging
import serial
import hashlib
import zlib
import time

from .. import FlashArgv, ProgressHandler, FlashHandler
from .esptool.targets import CHIP_DEFS, STUB_DEFS
from .esptool.loader import (
    DEFAULT_TIMEOUT,
    ERASE_WRITE_TIMEOUT_PER_MB,
    timeout_per_mb,
)


DETECTED_FLASH_SIZES = {
    0x12: "256KB",
    0x13: "512KB",
    0x14: "1MB",
    0x15: "2MB",
    0x16: "4MB",
    0x17: "8MB",
    0x18: "16MB",
    0x19: "32MB",
    0x1A: "64MB",
    0x1B: "128MB",
    0x1C: "256MB",
    0x20: "64MB",
    0x21: "128MB",
    0x22: "256MB",
    0x32: "256KB",
    0x33: "512KB",
    0x34: "1MB",
    0x35: "2MB",
    0x36: "4MB",
    0x37: "8MB",
    0x38: "16MB",
    0x39: "32MB",
    0x3A: "64MB",
}


class ESPFlashHandler(FlashHandler):
    def __init__(self,
                 argv: FlashArgv,
                 logger: logging.Logger = None,
                 progress: ProgressHandler = None):
        super().__init__(argv, logger, progress)
        self.esp = None
        self.esp_initial_baud = 115200
        self.binfile_data = {}
        self.ser = serial.Serial(self.port, self.esp_initial_baud, timeout=0.1)
        pass

    def serial_close(self):
        self.ser.close()
        pass

    def check_stop(self):
        return self.stop_flag

    def _pad_to(self, data, alignment, pad_character=b"\xFF"):
        pad_mod = len(data) % alignment
        if pad_mod != 0:
            data += pad_character * (alignment - pad_mod)
        return data

    def binfile_prepare(self):
        if len(self.binfile_data):
            return True
        argfile = open(self.binfile, 'rb')
        uncimage = self._pad_to(argfile.read(), 4)
        uncsize = len(uncimage)
        if uncsize == 0:
            self.logger.warning(f"File {self.binfile} is empty")
            return False
        calcmd5 = hashlib.md5(uncimage).hexdigest()
        self.binfile_data['uncimage'] = uncimage
        self.binfile_data['uncsize'] = uncsize
        self.binfile_data['calcmd5'] = calcmd5
        return True

    def shake(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        if not self.binfile_prepare():
            return False
        chip_class = CHIP_DEFS[self.device.lower()]
        self.esp = chip_class(self.ser, self.logger)
        self.logger.info("Connecting ...")
        if not self.esp.connect(self.check_stop):
            self.logger.error("Shake failed.")

        self.logger.info("Shake success.")
        return True

    def erase(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        if self.stop_flag:
            return False
        stub_flasher = STUB_DEFS[self.device.lower()]
        self.esp = self.esp.run_stub(stub_flasher)
        if self.esp is None:
            self.logger.error("Stub flash failed")
            return False

        self.logger.info("Stub flash success")
        return True

    def _set_baudrate(self, baud):
        if baud > self.esp_initial_baud:
            if self.esp.change_baud(baud):
                self.logger.info(f"Set baudrate [{baud}] success.")
            else:
                self.logger.error(f"Set baudrate [{baud}] fail.")
                return False
        return True

    def _detect_flash_size(self):
        flash_id = self.esp.flash_id()
        self.logger.debug(f"flash_id: {flash_id}")
        if flash_id is None:
            return "4MB"
        size_id = flash_id >> 16
        self.logger.debug(f"size_id: {size_id}")
        flash_size = DETECTED_FLASH_SIZES.get(size_id, "4MB")
        self.logger.debug(f"flash_size: {flash_size}")
        return flash_size

    def write(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        if not self._set_baudrate(self.baudrate):
            return False

        if self.stop_flag:
            return False
        flash_size_str = self._detect_flash_size()
        flash_size = self.esp.flash_size_bytes(flash_size_str)
        self.esp.flash_set_parameters(flash_size)

        binfile_size = os.path.getsize(self.binfile)
        if self.start_addr + binfile_size > flash_size:
            self.logger.error(
                f"File {self.binfile} (length {binfile_size}) \
at offset {self.start_arrd} "
                f"will not fit in {flash_size} bytes of flash. "
            )
            return False

        if not self.binfile_prepare():
            return False

        if self.stop_flag:
            return False
        uncsize = self.binfile_data['uncsize']
        uncimage = self.binfile_data['uncimage']
        image = zlib.compress(uncimage, 9)
        decompress = zlib.decompressobj()
        blocks = 1 + self.esp.flash_defl_begin(uncsize,
                                               len(image),
                                               self.start_addr)

        seq = 0
        timeout = DEFAULT_TIMEOUT

        self.progress.setup("Writing", blocks)
        self.progress.start()

        while len(image) > 0:
            if self.stop_flag:
                self.progress.close()
                return False
            block = image[0:self.esp.FLASH_WRITE_SIZE]
            block_uncompressed = len(decompress.decompress(block))
            block_timeout = max(
                DEFAULT_TIMEOUT,
                timeout_per_mb(ERASE_WRITE_TIMEOUT_PER_MB, block_uncompressed),
            )
            if not self.esp.flash_defl_block(block, seq, timeout=timeout):
                self.progress.close()
                return False
            self.progress.update()
            timeout = block_timeout
            image = image[self.esp.FLASH_WRITE_SIZE:]
            seq += 1

        self.esp.read_reg(self.esp.CHIP_DETECT_MAGIC_REG_ADDR, timeout=timeout)
        self.progress.update()
        self.esp.flash_begin(0, 0)
        self.esp.flash_defl_finish(False)

        time.sleep(0.1)  # 进度条显示完整
        self.progress.close()

        self.logger.info("Write flash success")
        return True

    def crc_check(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        if self.stop_flag:
            return False
        if not self.binfile_prepare():
            return False
        self.progress.setup("CRCChecking", 3)
        self.progress.start()

        self.progress.update()
        res = self.esp.flash_md5sum(self.start_addr,
                                    self.binfile_data['uncsize'])

        self.progress.update()
        calcmd5 = self.binfile_data['calcmd5']
        self.logger.debug("File  md5: %s" % calcmd5)
        self.logger.debug("Flash md5: %s" % res)
        if res != calcmd5:
            self.logger.error(f"Check CRC fail -> \n\
binfile_md5: {calcmd5} != flash_md5: {res}")
            return False

        self.progress.update()
        time.sleep(0.1)  # 进度条显示完整
        self.progress.close()
        self.logger.info("CRC check success")
        return True

    def reboot(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        if self.esp is None:
            return False
        self.esp.hard_reset()
        self.logger.info("Reboot done")
        return True

    def read(self, length):
        self.logger.debug(sys._getframe().f_code.co_name)
        self.logger.error("Don't support read.")
        return False
