#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import time
import logging
import serial
import platform

from .. import FlashArgv, ProgressHandler, FlashHandler
from .protocol import BUF, CRC, FLASH


def slip_reader(port, rxlen, timeout):
    waiting_head = True  # False: receive body
    timer = serial.Timeout(timeout)
    read_buf = bytearray()
    while not timer.expired():
        waiting = port.inWaiting()
        ser_buf = port.read(1 if waiting == 0 else waiting)
        read_buf += bytearray(ser_buf)
        if waiting_head:
            head = read_buf.find(b"\x04")
            head = 0 if head < 0 else head
            read_buf = read_buf[head:]  # remove useless buf
            if len(read_buf) < 7:  # minimum
                continue
            if read_buf[0:2] == b"\x04\x0e" \
                    and read_buf[3:6] == b"\x01\xe0\xfc":  # header
                waiting_head = False
                continue
            read_buf.pop(0)  # next
        if not waiting_head:
            if len(read_buf) >= rxlen:
                yield read_buf
    if len(read_buf) < rxlen:
        yield b""
    yield read_buf


class BKFlashSerial(object):
    def __init__(self,
                 port: str,
                 baudrate: int,
                 ser_timeout: float = 0.05):
        self.serial = serial.Serial(port, baudrate, timeout=ser_timeout)
        _env = platform.system().lower()
        if "darwin" in _env:  # mac
            self.TX = self._tx_mac
        else:
            self.TX = self._tx_normal
        pass

    def _tx_mac(self, buf: str) -> None:
        size = 256
        total = len(buf)
        count = int(total / size)
        rem = total % size
        for i in range(count):
            self.serial.write(buf[i*size:(i+1)*size])
            time.sleep(20/1000/2)
        if rem:
            self.serial.write(buf[-rem:])
            time.sleep(20/1000/2)
        pass

    def _tx_normal(self, buf: str) -> None:
        self.serial.write(buf)
        pass

    def close(self):
        self.serial.close()
        pass

    def RX(self,
           rxlen: int,
           process_timeout: float = 0.5,
           ser_timeout: float = 0.05) -> bytearray:
        self.serial.timeout = ser_timeout
        if self.reader is not None:
            return next(self.reader)
        return b""

    def RXWithTx(self,
                 txbuf: bytearray,
                 rxlen: int,
                 process_timeout: float = 0.5,
                 ser_timeout: float = 0.05) -> bytearray:
        self.reader = slip_reader(self.serial, rxlen, process_timeout)
        self.TX(txbuf)
        rxbuf = self.RX(rxlen, process_timeout, ser_timeout)
        return rxbuf

    def Flush(self) -> None:
        self.serial.flushInput()  # clear read buf
        self.serial.flushOutput()  # stop write and clear write buf
        pass

    def ReadSector(self, addr: int) -> bytearray:
        txbuf = BUF.FlashRead4K(addr)
        rxlen = BUF.CalcRxLength_FlashRead4K()
        rxbuf = self.RXWithTx(txbuf, rxlen, process_timeout=5)
        if rxbuf:
            ret, _, data = BUF.CheckRespond_FlashRead4K(rxbuf, addr)
            if ret:
                return data
        return b''

    def EraseBlock(self, addr: int, val: int) -> bool:
        txbuf = BUF.FlashErase(addr, val)
        rxlen = BUF.CalcRxLength_FlashErase()
        rxbuf = self.RXWithTx(txbuf, rxlen, process_timeout=1)
        ret = False
        if rxbuf:
            ret, _ = BUF.CheckRespond_FlashErase(rxbuf, addr, val)
        return ret

    def WriteSector(self, addr: int, buf: bytearray) -> bool:
        txbuf = BUF.FlashWrite4K(addr, buf)
        rxlen = BUF.CalcRxLength_FlashWrite4K()
        rxbuf = self.RXWithTx(txbuf, rxlen)
        ret, _ = BUF.CheckRespond_FlashWrite4K(rxbuf, addr)
        return ret

    def WriteSectorLoops(self, addr: int, buf: bytearray, loop: int) -> bool:
        txbuf = BUF.FlashWrite4K(addr, buf)
        rxlen = BUF.CalcRxLength_FlashWrite4K()
        ret = False
        for _ in range(loop):
            rxbuf = self.RXWithTx(txbuf, rxlen)
            ret, _ = BUF.CheckRespond_FlashWrite4K(rxbuf, addr)
            if ret:
                break
        return ret

    def ReadCRC(self, start: int, end: int, timeout=5) -> (bool, int):
        txbuf = BUF.CheckCRC(start, end)
        rxlen = BUF.CalcRxLength_CheckCRC()
        rxbuf = self.RXWithTx(txbuf, rxlen, process_timeout=timeout)
        return BUF.CheckRespond_CheckCRC(rxbuf, start, end)

    def GetFlashMID(self):
        txbuf = BUF.FlashGetMID(0x9f)
        rxlen = BUF.CalcRxLength_FlashGetID()
        rxbuf = self.RXWithTx(txbuf, rxlen)
        if rxbuf:
            return BUF.CheckRespond_FlashGetMID(rxbuf)
        return 0

    def ReadFlashSR(self, regAddr):
        txbuf = BUF.FlashReadSR(regAddr)
        rxlen = BUF.CalcRxLength_FlashReadSR()
        rxbuf = self.RXWithTx(txbuf, rxlen)
        if rxbuf:
            return BUF.CheckRespond_FlashReadSR(rxbuf, regAddr)
        return False, 0, 0

    def WriteFlashSR(self, sz, regAddr, value):
        if sz == 1:
            txbuf = BUF.FlashWriteSR(regAddr, value)
            rxlen = BUF.CalcRxLength_FlashWriteSR()
        else:
            txbuf = BUF.FlashWriteSR2(regAddr, value)
            rxlen = BUF.CalcRxLength_FlashWriteSR2()
        rxbuf = self.RXWithTx(txbuf, rxlen)
        if rxbuf:
            if sz == 1:
                return BUF.CheckRespond_FlashWriteSR(rxbuf, regAddr, value)
            else:
                return BUF.CheckRespond_FlashWriteSR2(rxbuf, regAddr, value)
        return False, None


class BK7231NFlashHandler(FlashHandler):
    def __init__(self,
                 argv: FlashArgv,
                 logger: logging.Logger = None,
                 progress: ProgressHandler = None):
        super().__init__(argv, logger, progress)
        self.serial = BKFlashSerial(argv.port, 115200, 0.001)
        self.binfil = {}
        self.retry = 4
        pass

    def binfile_prepare(self):
        with open(self.binfile, "rb") as f:
            bin_data = f.read()
        file_len = len(bin_data)
        self.logger.debug(f'binfile len: {file_len}')
        padding_len = 0x100 - (file_len & 0xff)
        bin_data += b'\xff' * padding_len
        file_len += padding_len
        self.logger.debug(f'binfile len after padding: {file_len}')

        self.binfil['bin'] = bin_data
        self.binfil['len'] = file_len
        pass

    def reboot_cmd_tx(self):
        for i in range(3):
            self.serial.TX(BUF.BKRegDoReboot())
            time.sleep(0.15)
        self.serial.serial.dtr = 0
        self.serial.serial.rts = 1
        time.sleep(0.2)
        self.serial.serial.rts = 0
        self.serial.TX(BUF.BKRegDoReboot())

    def serial_close(self):
        self.serial.close()
        pass

    def shake(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        # Step: Try auto reboot
        self.reboot_cmd_tx()

        # Step: Link Check
        count_sec = 0
        count = 0
        txbuf = BUF.LinkCheck()
        rxlen = BUF.CalcRxLength_LinkCheck()
        self.logger.info("Waiting Reset ...")
        while True:
            if self.stop_flag:
                return False
            rxbuf = self.serial.RXWithTx(txbuf, rxlen, 0.001, 0.001)
            if rxbuf and BUF.CheckRespond_LinkCheck(rxbuf):
                break
            if count_sec > 100:
                self.logger.debug("link check try again ...")
                self.reboot_cmd_tx()
                count_sec = 0
                count += 1
                if count > 15:
                    self.logger.warning("Shake timeout!")
                    return False
                self.serial.TX(BUF.BKRegDoReboot())

                if self.serial.serial.baudrate == 115200:
                    self.serial.serial.baudrate = 921600
                elif self.serial.serial.baudrate == 921600:
                    self.serial.serial.baudrate = 115200

                # Send reboot via command line
                self.serial.TX(b"reboot\r\n")

                # reset to bootrom baudrate
                if self.serial.serial.baudrate != 115200:
                    self.serial.serial.baudrate = 115200
            count_sec += 1
        self.logger.info("link check success")

        time.sleep(0.01)
        self.serial.Flush()
        if self.stop_flag:
            return False

        # Step: Sync Baudrate
        if self.baudrate != 115200:
            dly_ms = 100
            txbuf = BUF.SetBaudRate(self.baudrate, dly_ms=dly_ms)
            rxlen = BUF.CalcRxLength_SetBaudRate()
            self.serial.TX(txbuf)
            time.sleep(20/1000/2)
            self.serial.serial.baudrate = self.baudrate
            self.serial.reader = slip_reader(self.serial.serial, rxlen, 0.5)

            rxbuf = self.serial.RX(rxlen, 0.5)
            if rxbuf and BUF.CheckRespond_SetBaudRate(
                    rxbuf, self.baudrate, dly_ms=dly_ms):
                self.logger.debug(f'set baudrate {self.baudrate} success')
            else:
                self.logger.warning(f'set baudrate {self.baudrate} fail')
                return False
        self.logger.info(f'sync baudrate {self.baudrate} success')

        return True

    def erase(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        if self.stop_flag:
            return False
        self.binfile_prepare()
        binfile_len = self.binfil['len']
        binfile_data = self.binfil['bin']
        binfile_sub_len = binfile_len

        self.progress.setup("Erasing", 5)
        self.progress.start()
        self.progress.update()

        # Get flash MID
        for _ in range(self.retry):
            flash_mid = self.serial.GetFlashMID()
            if flash_mid:
                self.logger.debug(f'flash_mid: {flash_mid}')
                self.flash_mid = flash_mid
                break
        else:
            self.logger.error("Get flash MID failed")
            return False

        # Unprotect Flash
        for _ in range(self.retry):
            if self._Do_Boot_ProtectFlash(flash_mid, True):
                self.logger.debug("Unprotect flash success")
                break
            time.sleep(0.0001)
        else:
            self.logger.error("Unprotect flash failed")
            return False

        # Step: Erase first 4K
        if self.stop_flag:
            return False
        erase_addr = self.start_addr  # if 0x1234
        aligned_addr = erase_addr & 0xfffff000  # 0x1000  # 按4K对齐(也是写的起始地址)
        aligned_out_len = erase_addr - aligned_addr  # 0x0234  # 相对于对齐位置的多余部分
        if aligned_out_len:
            self.logger.debug(
                f'erase first 4K block [{aligned_addr}:{aligned_out_len}] ...')
            buf4K = self.serial.ReadSector(aligned_addr)  # 首个4K数据内容
            if len(buf4K) < 4096:
                self.logger.error(f'Read sector not enough 4K: [{len(buf4K)}]')
                self.logger.info("You can try other baudrate.")
                return False
            if buf4K:
                cover_len = 0x1000 - aligned_out_len  # 0xdcc # 首个4K中需要被烧录的长度
                # 如果bin很短就只覆盖bin需要的部分
                cover_len_min = cover_len if \
                    cover_len < binfile_len else binfile_len
                buf4K[aligned_out_len:aligned_out_len+cover_len_min] = \
                    binfile_data[:cover_len_min]  # 将bin开头内容覆盖上去
                self.serial.EraseBlock(aligned_addr, 0x20)  # 擦除第一个4K
                ans = self.serial.WriteSector(aligned_addr, buf4K)
                if not ans:
                    self.logger.error("erase first 4K block fail")
                    self.progress.close()
                    return False
                binfile_sub_len -= cover_len_min
                if binfile_sub_len <= 0:
                    self.progress.close()
                    return True
                binfile_data = binfile_data[cover_len_min:]  # 删除已经写入过的内容
                aligned_addr += 0x1000
        self.progress.update()

        # Step: Erase last 4K
        if self.stop_flag:
            return False
        erase_end_addr = erase_addr + binfile_len  # 0x1234 + 0x8765 = 0x9999
        aligned_end_addr = erase_end_addr & 0xfffff000  # 0x9000
        # 相对于对齐位置的多余部分
        aligned_out_len = erase_end_addr - aligned_end_addr  # 0x0999
        if aligned_out_len:
            self.logger.debug(f'erase last 4K block \
[{aligned_end_addr}:{aligned_out_len}] ...')
            buf4K = bytearray(b'\xff' * 4096)
            # 将bin结尾部分覆盖上去
            buf4K[:aligned_out_len] = binfile_data[-aligned_out_len:]
            self.serial.EraseBlock(aligned_end_addr, 0x20)  # 擦除最后一个4K
            ans = self.serial.WriteSectorLoops(aligned_end_addr,
                                               buf4K, self.retry)
            if not ans:
                self.logger.error("erase last 4K block fail")
                self.progress.close()
                return False
            binfile_sub_len -= aligned_out_len
            if binfile_sub_len <= 0:
                self.progress.close()
                return True
            binfile_data = binfile_data[:-aligned_out_len]  # 删除已经写入过的内容
        self.progress.update()

        # Step: Erase other
        if self.stop_flag:
            return False
        erase_start_addr = aligned_addr  # if 0x12000  # Step1中已经处理
        aligned_64K_addr = erase_start_addr & 0xffff0000  # 0x10000  # 按64K对齐
        # 相对于对齐位置的多余部分
        aligned_out_len = erase_start_addr - aligned_64K_addr  # 0x02000
        if aligned_out_len:
            aligned_64K_addr += 0x10000  # 跨到下一个64K对齐
        if binfile_sub_len > aligned_64K_addr - erase_start_addr:
            while erase_start_addr < aligned_64K_addr:  # 将跨过的部分擦除
                if self.stop_flag:
                    return False
                self.logger.debug(
                    f'erase other block (1/2) [{erase_start_addr}:4K] ...')
                self.serial.EraseBlock(erase_start_addr, 0x20)  # 4K擦除
                erase_start_addr += 0x1000
        self.progress.update()

        erase_end_addr = aligned_addr + binfile_sub_len
        erase_now_addr = erase_start_addr
        while erase_now_addr < erase_end_addr:
            if self.stop_flag:
                return False
            sub_len = erase_end_addr - erase_now_addr
            if sub_len > 0x10000:
                self.logger.debug(
                    f'erase other block (2/2) [{erase_now_addr}:64K] ...')
                self.serial.EraseBlock(erase_now_addr, 0xd8)  # 64K
                erase_now_addr += 0x10000
            else:
                self.logger.debug(
                    f'erase other block (2/2) [{erase_now_addr}:4K] ...')
                self.serial.EraseBlock(erase_now_addr, 0x20)  # 4K
                erase_now_addr += 0x1000
        self.progress.update()
        time.sleep(0.1)  # 进度条显示完整
        self.binfil['sub_bin'] = binfile_data
        self.binfil['start_addr'] = aligned_addr

        self.progress.close()
        self.logger.info("Erase flash success")
        return True

    def write(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        if self.stop_flag:
            return False
        bin_data = self.binfil['sub_bin']
        write_start_addr = self.binfil['start_addr']
        binfile_sub_len = len(bin_data)
        total_write = binfile_sub_len // 0x1000
        write_end_addr = write_start_addr + binfile_sub_len

        self.progress.setup("Writing", total_write)
        self.progress.start()
        self.logger.debug(f'write_start_addr: {write_start_addr}')
        self.logger.debug(f'write_end_addr: {write_end_addr}')
        self.logger.debug(f'binfile_sub_len: {binfile_sub_len}')
        self.logger.debug(f'total_write: {total_write}')

        write_now_addr = write_start_addr
        bin_data_ptr = 0
        while write_now_addr < write_end_addr:
            if self.stop_flag:
                return False
            self.logger.debug(f'write flash [{write_now_addr}] ...')
            if not self.serial.WriteSectorLoops(
                    write_now_addr,
                    bin_data[bin_data_ptr:bin_data_ptr+0x1000], self.retry):
                self.progress.close()
                self.logger.error("Write flash fail!")
                return False
            self.progress.update()
            write_now_addr += 0x1000
            bin_data_ptr += 0x1000

        self._Do_Boot_ProtectFlash(self.flash_mid, False)

        # self.progress.update()
        time.sleep(0.1)  # 进度条显示完整
        self.progress.close()
        self.logger.info("Write flash success")
        return True

    def crc_check(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        if self.stop_flag:
            return False
        bin_data = self.binfil['bin']
        bin_len = self.binfil['len']
        start_addr = self.start_addr
        self.progress.setup("CRCChecking", 4)
        self.progress.start()

        self.progress.update()
        calc_crc = CRC()
        bin_crc = calc_crc.crc32(0xffffffff, bin_data)
        self.logger.debug(f'bin_crc: {bin_crc}')

        self.progress.update()
        aligned_len = bin_len
        if bin_len & 0xff:
            aligned_len = (bin_len & ~0xff) + 0x100  # 向上对齐

        timeout = bin_len * 15 / 1024 / 1024
        if timeout < 15:
            timeout = 15

        self.progress.update()
        ans, rx_crc = self.serial.ReadCRC(start_addr,
                                          start_addr+aligned_len-1,
                                          timeout=timeout)
        self.logger.debug(f'rx_crc: {rx_crc}')
        if not ans:
            self.logger.error("Read CRC fail!")
            self.progress.close()
            return False
        if rx_crc != bin_crc:
            self.logger.warning(f'Check CRC fail -> \n\
binfile_crc: {bin_crc} != readchip_crc: {rx_crc}')
            self.progress.close()
            return False

        self.progress.update()
        time.sleep(0.1)  # 进度条显示完整
        self.progress.close()
        self.logger.info("CRC check success")
        return True

    def reboot(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        txbuf = BUF.Reboot()
        for _ in range(3):
            self.serial.TX(txbuf)
        self.progress.close()
        self.logger.info("Reboot done")
        return True

    def read(self, length):
        self.logger.debug(sys._getframe().f_code.co_name)
        if self.stop_flag:
            return False
        start_addr = self.start_addr
        binbuf = bytearray()
        read_addr = start_addr & 0xfffff000  # 对齐4K
        read_length = 0
        self.logger.debug(f'start_addr: {start_addr}')
        self.logger.debug(f'read_addr: {read_addr}')
        self.logger.debug(f'length: {length}')

        total_read = length // 0x1000
        self.progress.setup("Reading", total_read)
        self.progress.start()

        less_count = 0
        while read_length < length:
            if self.stop_flag:
                return False
            rxbuf = self.serial.ReadSector(read_addr + read_length)
            rxlen = len(rxbuf)
            if rxlen < 4096:
                self.logger.warning(f'Read sector not enough 4K: [{rxlen}]')
                less_count += 1
                if less_count >= 10:
                    self.logger.error("Read failure many times.")
                    self.logger.info("You can try other baudrate.")
                    return False

                continue
            less_count = 0
            binbuf += rxbuf
            self.progress.update()
            read_length += 0x1000
        self.logger.debug(f'read_length: {read_length}')

        self.progress.update()
        time.sleep(0.1)  # 进度条显示完整
        self.progress.close()

        self.logger.debug("writing bin to file")
        with open(self.binfile, 'wb') as f:
            offset_addr = start_addr - read_addr
            f.write(binbuf[offset_addr:offset_addr + length])

        self.binfile_prepare()
        self.logger.info("Read flash success")
        return True

    def _Do_Boot_ProtectFlash(self, mid: int, unprotect: bool):
        # 1. find flash info
        flash_info = FLASH.GetFlashInfo(mid)
        if flash_info is None:
            return False
        self.logger.debug(f'flash_info: {flash_info}')

        timeout = 50

        # 2. write (un)protect word
        cw = flash_info.cwUnp if unprotect else flash_info.cwEnp
        loop_cnt = 0
        while True:
            if self.stop_flag:
                return False
            sr = 0
            loop_cnt += 1
            # read sr register
            for i in range(flash_info.szSR):
                f = self.serial.ReadFlashSR(flash_info.cwdRd[i])
                if f[0]:
                    sr |= f[2] << (8 * i)
            # if (un)protect word is set
            if (sr & flash_info.cwMsk) == FLASH.CalcBFD(cw,
                                                        flash_info.sb,
                                                        flash_info.lb):
                return True
            if loop_cnt > timeout:
                return False
            # // set (un)protect word
            srt = sr & (flash_info.cwMsk ^ 0xffffffff)
            srt |= FLASH.CalcBFD(cw, flash_info.sb, flash_info.lb)
            f = self.serial.WriteFlashSR(flash_info.szSR,
                                         flash_info.cwdWr[0],
                                         srt & 0xffff)

            time.sleep(0.01)

        return True
