#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import time
import logging

from .. import FlashArgv, ProgressHandler, FlashHandler
from .boot_intf import CBootIntf
from .crc32v2 import *
from .flash_list import *
from serial import Timeout


class BKEXFlashHandler(FlashHandler):
    def __init__(self,
                 argv: FlashArgv,
                 logger: logging.Logger = None,
                 progress: ProgressHandler = None):
        super().__init__(argv, logger, progress)
        self.bootItf = CBootIntf(argv.port, 115200, 0.001)
        self.binfil = {}
        pass

    def binfile_prepare(self):
        # Step1: read file into system memory
        # TODO: sanity check
        filename = self.binfile
        with open(filename, "rb") as f:
            pfile = f.read()
        fileLen = len(pfile)
        padding_len = 0x100 - (fileLen & 0xff)
        if padding_len:
            pfile += b'\xff'*padding_len
            fileLen += padding_len

        self.binfil['bin'] = pfile
        self.binfil['len'] = fileLen
        pass


    def shake(self):
        self.logger.debug(sys._getframe().f_code.co_name)

        self.do_reset_signal()
        timeout = Timeout(10)
        self.logger.info("Waiting Reset ...")

        # Step2: Link Check
        count = 0
        # Send reboot via bkreg
        self.bootItf.SendBkRegReboot()
        while True:
            r = self.bootItf.LinkCheck()
            if r:
                break
            if timeout.expired():
                self.logger.warning("Shake timeout!")
                return False
            count += 1
            if count > 20:
                self.logger.debug("link check try again ...")
                # Send reboot via bkreg
                self.bootItf.SendBkRegReboot()

                if self.bootItf.ser.baudrate == 115200:
                    self.bootItf.ser.baudrate = 921600
                elif self.bootItf.ser.baudrate == 921600:
                    self.bootItf.ser.baudrate = 115200

                # Send reboot via command line
                self.bootItf.Start_Cmd(b"reboot\r\n")

                # reset to bootrom baudrate
                if self.bootItf.ser.baudrate != 115200:
                    self.bootItf.ser.baudrate = 115200
                count = 0

        self.logger.info("link check success")
        time.sleep(0.01)
        self.bootItf.Drain()

        # Step3: set baudrate, delay 100ms
        if self.baudrate != 115200:
            if not self.bootItf.SetBR(self.baudrate, 20):
                self.logger.warning(f'set baudrate {self.baudrate} fail')
                return False

        self.logger.info(f'sync baudrate {self.baudrate} success')
        return True


    def erase(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        self.binfile_prepare()
        startAddr = self.start_addr
        pfile = self.binfil['bin']
        fileLen = self.binfil['len']
        filOLen = fileLen
        self.progress.setup("Erasing", 5)
        self.progress.start()
        self.progress.update()

        # Get Mid
        mid = self.bootItf.GetFlashMID()
        # print("\n\n mid: {:x}\n\n".format(mid))
        self.flash_mid = mid
        if self._Do_Boot_ProtectFlash(mid, True) != 1:
            self.logger.erroe("Unprotect Failed")
            return False
        # unprotect flash first

        # Step4: erase
        # Step4.1: read first 4k if startAddr not aligned with 4K
        eraseAddr = startAddr
        ss = s0 = eraseAddr & 0xfffff000     # 4K对齐的地址
        filSPtr = 0

        if eraseAddr & 0xfff:
            self.logger.debug(f'erase first 4K block [{ss}:{eraseAddr&0xfff}] ...')
            # Read 4K from flash
            buf = self.bootItf.ReadSector(s0)
            l = 0x1000 - (eraseAddr & 0xfff)
            fl = l if l < fileLen else fileLen
            buf = bytearray(buf)
            buf[eraseAddr&0xfff:eraseAddr&0xfff+fl] = pfile[:fl]
            self.bootItf.EraseBlock(0x20, s0)
            if not self.bootItf.WriteSector(s0, buf):
                self.logger.error("erase first 4K block fail")
                self.progress.close()
                return False
            filOLen -= fl
            filSPtr = fl
            s0 += 0x1000
            ss = s0

            if filOLen <= 0:
                self.progress.close()
                return True
        self.progress.update()

        # Step4.2: handle the last 4K
        # now ss is the new eraseAddr.
        # 文件结束地址
        filEPtr = fileLen
        s1 = eraseAddr + fileLen

        # If not 4K aligned, read the last sector, fill the data, write it back
        if s1 & 0xfff:
            self.logger.debug(f'erase last 4K block [{s1&0xfffff000}:{s1&0xfff}] ...')
            buf = bytearray(b'\xff'*4096)   # fill with 0xFF
            buf[:s1&0xfff] = pfile[filEPtr-(s1&0xfff):]  # copy s1&0xfff len
            for _ in range(4):
                if self.bootItf.EraseBlock(0x20, s1&0xfffff000):
                    break
                time.sleep(0.05)

            if not self.bootItf.WriteSector(s1&0xfffff000, buf):
                self.logger.error("erase last 4K block fail")
                self.progress.close()
                return False

            filEPtr = filEPtr - (s1&0xfff)
            filOLen = filOLen - (s1&0xfff)
            if filOLen <= 0:
                self.progress.close()
                return True
        self.progress.update()

        # Step4.3: 对齐64KB，如果擦除区域小于64KB
        len1 = ss + filOLen
        len0 = s0 & 0xffff0000
        if s0 & 0xffff:
            len0 += 0x10000
        if filOLen > len0 - s0:
            while s0 < len0:
                self.logger.debug(f'erase other block (1/2) [{s0}:4K] ...')
                self.bootItf.EraseBlock(0x20, s0)
                s0 += 0x1000
        self.progress.update()

        # Step4.4: Erase 64k, 4k, etc.
        # 按照64KB/4K block擦除
        self.logger.debug(f'fileOLen: {filOLen}, filSPtr: {filSPtr}')
        len1 = ss + filOLen
        while s0 < len1:
            rmn = len1 - s0
            if rmn > 0x10000:   # 64K erase
                self.logger.debug(f'erase other block (2/2) [{s0}:64K] ...')
                self.bootItf.EraseBlock(0xd8, s0)
                s0 = s0+0x10000
            else:       # erase 4K
                self.logger.debug(f'erase other block (2/2) [{s0}:4K] ...')
                self.bootItf.EraseBlock(0x20, s0)
                s0 = s0 + 0x1000
        self.progress.update()
        time.sleep(0.1)  # 进度条显示完整

        self.progress.close()
        self.binfil['filSPtr'] = filSPtr
        self.binfil['filOLen'] = filOLen
        self.binfil['ss'] = ss
        self.logger.info(f'Erase flash success')
        return True


    def write(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        pfile = self.binfil['bin']
        filSPtr = self.binfil['filSPtr']
        filOLen = self.binfil['filOLen']
        ss = self.binfil['ss']

        binfile_sub_len = len(pfile)
        total_write = binfile_sub_len // 0x1000
        self.progress.setup("Writing", total_write)
        self.progress.start()
        # Step5: Write data
        i = 0
        while i < filOLen:
            self.logger.debug(f'write flash [{ss+i}] ...')
            for _ in range(4):
                if self.bootItf.WriteSector(ss+i, pfile[filSPtr+i:filSPtr+i+4*1024]):
                    self.progress.update()
                    break
            else:
                self.progress.close()
                self.logger.error("Write flash fail!")
                return False
            i += 0x1000

        mid = self.flash_mid
        self._Do_Boot_ProtectFlash(mid, False)

        self.progress.update()
        time.sleep(0.1)  # 进度条显示完整
        self.progress.close()
        self.logger.info(f'Write flash success')
        return True


    def crc_check(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        pfile = self.binfil['bin']
        fileLen = self.binfil['len']
        startAddr = self.start_addr
        self.progress.setup("CRCChecking", 4)
        self.progress.start()

        self.progress.update()
        fileCrc = crc32_ver2(0xffffffff, pfile)
        self.logger.debug(f'binfile_crc: {fileCrc}')

        # Step6: CRC check
        self.progress.update()
        if fileLen & 0xff:
            l2 = (fileLen & ~0xff) + 0x100
        else:
            l2 = fileLen
        crcTo = fileLen * 15 / 1024 / 1024
        if crcTo < 15:
            crcTo = 15
        self.progress.update()
        ret, crc = self.bootItf.ReadCRC(startAddr, startAddr+l2-1, crcTo)
        self.logger.debug(f'readchip_crc: {crc}')
        if not ret:
            self.logger.error("Read CRC fail!")
            self.progress.close()
            return False
        if crc != fileCrc:
            self.logger.warning(f'Check CRC fail -> \nbinfile_crc: {fileCrc} != readchip_crc: {crc}')
            self.progress.close()
            return False

        self.progress.update()
        time.sleep(0.1)  # 进度条显示完整
        self.progress.close()
        self.logger.info(f'CRC check success')
        return True


    def do_reset_signal(self):
        self.bootItf.ser.dtr = 0
        self.bootItf.ser.rts = 1
        time.sleep(0.2)
        self.bootItf.ser.rts = 0
        pass


    def reboot(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        for i in range(3):
            time.sleep(0.01)
            self.bootItf.SendReboot()
        self.logger.info(f'Reboot done')
        return True


    def read(self, length):
        self.logger.debug(sys._getframe().f_code.co_name)
        startAddr = self.start_addr
        readLength = length
        total_read = length // 0x1000
        self.progress.setup("Reading", total_read)
        self.progress.start()

        addr = startAddr & 0xfffff000
        count = 0
        buffer = bytes()

        while count < readLength:
            sector = self.bootItf.ReadSector(addr)
            if sector is not None:
                buffer += sector
            addr += 0x1000
            count += 0x1000
            self.progress.update()

        self.logger.debug(f'writing bin to file')
        with open(self.binfile, 'wb') as f:
            f.write(buffer[ : length])

        self.progress.update()
        time.sleep(0.1)  # 进度条显示完整
        self.progress.close()

        self.binfile_prepare()
        self.logger.info(f'Read flash success')
        return True


    def _Do_Boot_ProtectFlash(self, mid:int, unprotect:bool):
        # 1. find flash info
        flash_info = GetFlashInfo(mid)
        if flash_info is None:
            return -1

        timeout = Timeout(1)

        # 2. write (un)protect word
        cw = flash_info.cwUnp if unprotect else flash_info.cwEnp
        while True:
            sr = 0

            # read sr register
            for i in range(flash_info.szSR):
                f = self.bootItf.ReadFlashSR(flash_info.cwdRd[i])
                if f[0]:
                    sr |= f[2] << (8 * i)

            # if (un)protect word is set
            if (sr & flash_info.cwMsk) == BFD(cw, flash_info.sb, flash_info.lb):
                return 1
            if timeout.expired():
                return -2
            # // set (un)protect word
            srt = sr & (flash_info.cwMsk ^ 0xffffffff)
            srt |= BFD(cw, flash_info.sb, flash_info.lb)
            f = self.bootItf.WriteFlashSR(flash_info.szSR, flash_info.cwdWr[0], srt & 0xffff)

            time.sleep(0.01)

        return 1



