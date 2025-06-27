#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import time
import logging
import serial
import hashlib

from .. import FlashArgv, ProgressHandler, FlashHandler
from .protocol import BUF


class RTLFlashSerial(object):
    def __init__(self,
                 port: str,
                 baudrate: int,
                 ser_timeout: float = 0.05):
        self.serial = serial.Serial(port, baudrate, timeout=ser_timeout)
        pass

    def compare_respond(self, recv_buf: bytearray, respond: bytearray):
        window_size = len(respond)
        for i in range(len(recv_buf) - window_size + 1):
            if recv_buf[i:i + window_size] == respond:
                return True
        return False

    def close(self):
        self.serial.close()
        pass

    def TX(self, buf: str) -> None:
        self.serial.write(buf)
        pass

    def RX(self,
           rxlen,
           process_timeout: float = 0.5,
           ser_timeout: float = 0.05) -> bytearray:

        timer = serial.Timeout(process_timeout)
        self.serial.timeout = ser_timeout

        recv_buf = bytearray()

        while not timer.expired():
            ser_buf = self.serial.read(self.serial.in_waiting)

            recv_buf += ser_buf

            if len(recv_buf) >= rxlen:
                break

        return recv_buf

    def RX_Respond(self,
                   respond: bytearray,
                   process_timeout: float = 0.5,
                   ser_timeout: float = 0.05) -> bytearray:
        timer = serial.Timeout(process_timeout)
        self.serial.timeout = ser_timeout

        recv_buf = bytearray()
        ret = False

        while not timer.expired():
            ser_buf = self.serial.read(self.serial.in_waiting)   # read all
            recv_buf += bytearray(ser_buf)

            if len(recv_buf) >= len(respond):
                ret = self.compare_respond(recv_buf, respond)
                if ret:
                    break

        return ret

    def RXWithTx(self,
                 txbuf: bytearray,
                 respond: bytearray,
                 process_timeout: float = 0.5,
                 ser_timeout: float = 0.05) -> bytearray:
        self.TX(txbuf)

        ret = self.RX_Respond(respond, process_timeout, ser_timeout)
        return ret

    def Flush(self) -> None:
        self.serial.flushInput()  # clear read buf
        self.serial.flushOutput()  # stop write and clear write buf
        pass

    def WriteStart(self, chip_type) -> bool:
        txbuf, respond = BUF.FlashWrite_Start(chip_type)

        ret = self.RXWithTx(txbuf, respond, process_timeout=3, ser_timeout=0.1)

        return ret

    def WriteSector(self, index: int, buf: bytearray, loop: int) -> bool:
        txbuf, respond = BUF.FlashWrite_1K(index, buf)

        ret = False
        for _ in range(loop):
            ret = self.RXWithTx(txbuf, respond)
            if ret:
                break

        return ret

    def writeEnd(self) -> bool:
        txbuf, respond = BUF.FlashWrite_End()
        ret = self.RXWithTx(txbuf, respond, process_timeout=3, ser_timeout=0.1)

        return ret

    def CheckHash(self, bin_data: bytearray, file_len: int) -> bool:
        # sha256
        file_sha256 = hashlib.sha256(bin_data).hexdigest()

        txbuf, respond = BUF.FlashGet_Hash(file_len)
        self.TX(txbuf)

        recv_buf = self.RX(6 + 32, process_timeout=5)

        window_size = len(respond)
        ret = False
        recv_sha256 = b''
        for i in range(len(recv_buf) - window_size + 1):
            if recv_buf[i:i + window_size] == respond:
                recv_sha256 = recv_buf[i + 6: i + 6 + 32]
                if recv_sha256 == bytes.fromhex(file_sha256):
                    ret = True

                break

        return ret


class RTL8720CFlashHandler(FlashHandler):
    def __init__(self,
                 argv: FlashArgv,
                 logger: logging.Logger = None,
                 progress: ProgressHandler = None):
        super().__init__(argv, logger, progress)
        self.serial = RTLFlashSerial(argv.port, 115200, 0.001)
        self.binfil = {}
        self.chip_type = 0
        pass

    def binfile_prepare(self):
        with open(self.binfile, "rb") as f:
            bin_data = f.read()
        file_len = len(bin_data)
        self.logger.debug(f'binfile len: {file_len}')

        # 补齐0x1A
        if file_len % 1024:
            padding_len = 1024 - (file_len % 1024)
            bin_data += b'\x1A' * padding_len
            file_len += padding_len

        self.binfil['bin'] = bin_data
        self.binfil['len'] = file_len
        self.binfil['start_addr'] = self.start_addr
        pass

    def serial_close(self):
        self.serial.close()
        pass

    def shake(self):
        self.logger.debug(sys._getframe().f_code.co_name)

        self.serial.serial.dtr = 0
        self.serial.serial.rts = 1
        time.sleep(0.2)
        self.serial.serial.rts = 0

        self.logger.info("Waiting Reset ...")

        count_sec = 0
        count = 0

        txbuf = BUF.SetModule()
        respond_len = 61
        cf_respond = bytearray(61)
        cf_respond[:respond_len] = b'\x0D\x0A40000038:    00000020    \
00000000    000C0000    00000000\x0D\x0A'

        cm_respond = bytearray(61)
        cm_respond[:respond_len] = b'\x0D\x0A40000038:    00000000    \
00000000    000C0000    00000000\x0D\x0A'

        while True:
            if self.stop_flag:
                return False

            self.serial.TX(txbuf)

            process_timeout = 0.5
            ser_timeout = 0.05
            timer = serial.Timeout(process_timeout)
            self.serial.timeout = ser_timeout

            recv_buf = bytearray()
            self.chip_type = 0

            while not timer.expired():
                # read all
                ser_buf = self.serial.serial.read(
                            self.serial.serial.in_waiting
                            )
                recv_buf += bytearray(ser_buf)

                if len(recv_buf) >= len(cf_respond) and \
                        self.serial.compare_respond(recv_buf, cf_respond):
                    self.chip_type = 1              # RTL8720CF
                    break
                elif len(recv_buf) >= len(cm_respond) and \
                        self.serial.compare_respond(recv_buf, cm_respond):
                    self.chip_type = 2              # RTL8720CM
                    break

            if self.chip_type > 0:
                self.logger.debug("setmodule success!")
                break

            if count_sec > 500:
                self.logger.debug("link check try again ...")
                count_sec = 0
                count += 1
                if count > 15:
                    self.logger.warning("shake timeout")
                    return False

            count_sec += 1

        self.logger.debug("ping ...")
        # ping
        txbuf, respond = BUF.SendPing()
        ret = self.serial.RXWithTx(txbuf, respond)
        if not ret:
            return False

        self.logger.debug("Send ping success!")

        txbuf, respond = BUF.SetEW()
        ret = self.serial.RXWithTx(txbuf, respond)

        self.logger.info("link check success")

        time.sleep(0.01)
        self.serial.Flush()

        if self.stop_flag:
            return False

        # Sync Baudrate
        if self.baudrate != 115200:
            txbuf, respond = BUF.SetBaudRate(self.baudrate)

            self.serial.TX(txbuf)
            time.sleep(20/1000/2)
            self.serial.serial.baudrate = self.baudrate

            ret = self.serial.RX_Respond(respond, 5)
            if ret:
                self.logger.debug(f'set baudrate {self.baudrate} success')
            else:
                self.logger.error(f'set baudrate {self.baudrate} fail')
                return False

        self.logger.info(f'sync baudrate {self.baudrate} success')

        return True

    def erase(self):
        self.logger.debug(sys._getframe().f_code.co_name)

        if self.stop_flag:
            return False

        self.binfile_prepare()

        # self.progress.setup("Erasing", 5)
        # self.progress.start()
        # self.progress.update()

        # self.progress.close()

        # self.logger.info("Erase flash success")
        return True

    def write(self):
        self.logger.debug(sys._getframe().f_code.co_name)

        if self.stop_flag:
            return False

        # Transmit start
        if self.chip_type == 0:
            return False

        if not self.serial.WriteStart(self.chip_type):
            self.logger.error("write falsh start fail!")
            return False

        bin_data = self.binfil['bin']
        write_start_addr = self.binfil['start_addr']
        binfile_sub_len = len(bin_data)
        total_write = binfile_sub_len / 1024
        write_end_addr = write_start_addr + binfile_sub_len

        self.logger.debug(f'write_start_addr: {write_start_addr}')
        self.logger.debug(f'write_end_addr: {write_end_addr}')
        self.logger.debug(f'binfile_sub_len: {binfile_sub_len}')
        self.logger.debug(f'total_write: {total_write}')

        self.progress.setup('Writing', total_write)
        self.progress.start()

        write_now_addr = write_start_addr
        bin_data_ptr = 0

        while write_now_addr < write_end_addr:
            if self.stop_flag:
                return False

            self.logger.debug(f'write flash [{write_now_addr}] ...')
            if not self.serial.WriteSector(int(bin_data_ptr/1024) + 1,
                                           bin_data[
                                               bin_data_ptr:bin_data_ptr+1024
                                               ],
                                           4):
                self.progress.close()
                self.logger.error("write flash fail!")
                return False

            self.progress.update()
            write_now_addr += 1024
            bin_data_ptr += 1024

        self.progress.update()
        time.sleep(0.1)
        self.progress.close()

        if not self.serial.writeEnd():
            self.logger.error("write flash fail!")
            return False

        self.logger.info("Write flash success")
        return True

    def crc_check(self):
        self.logger.debug(sys._getframe().f_code.co_name)

        if self.stop_flag:
            return False

        with open(self.binfile, "rb") as f:
            bin_data = f.read()
        file_len = len(bin_data)

        ret = self.serial.CheckHash(bin_data, file_len)

        if not ret:
            self.logger.error("Hash check fail ...")
            return False

        self.logger.info("Hash check success")

        # set baudrate == 115200
        baudrate = 115200
        txbuf, respond = BUF.SetBaudRate(baudrate)

        self.serial.TX(txbuf)
        time.sleep(20/1000/2)
        self.serial.serial.baudrate = baudrate

        ret = self.serial.RX_Respond(respond, 5)
        if ret:
            self.logger.debug(f'set baudrate {baudrate} success')
        else:
            self.logger.error(f'set baudrate {baudrate} fail')
            return False

        return True

    def reboot(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        '''
        do something
        '''
        self.progress.close()
        # self.logger.info("Reboot done")
        return True

    def read(self, length):
        self.logger.debug(sys._getframe().f_code.co_name)
        '''
        do something
        '''
        self.logger.info("don't support read ...")
        return False
