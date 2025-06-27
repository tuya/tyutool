#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import logging
import serial
import os
import io
import time

from .. import FlashArgv, ProgressHandler, FlashHandler
from .ram_bin import RAM_BIN


SOH = b'\x01'
STX = b'\x02'
EOT = b'\x04'
ACK = b'\x06'
NAK = b'\x15'
CAN = b'\x18'
CRC = b'C'


class LN882HFlashSerial(object):
    crctable = [
        0x0000, 0x1021, 0x2042, 0x3063, 0x4084, 0x50a5, 0x60c6, 0x70e7,
        0x8108, 0x9129, 0xa14a, 0xb16b, 0xc18c, 0xd1ad, 0xe1ce, 0xf1ef,
        0x1231, 0x0210, 0x3273, 0x2252, 0x52b5, 0x4294, 0x72f7, 0x62d6,
        0x9339, 0x8318, 0xb37b, 0xa35a, 0xd3bd, 0xc39c, 0xf3ff, 0xe3de,
        0x2462, 0x3443, 0x0420, 0x1401, 0x64e6, 0x74c7, 0x44a4, 0x5485,
        0xa56a, 0xb54b, 0x8528, 0x9509, 0xe5ee, 0xf5cf, 0xc5ac, 0xd58d,
        0x3653, 0x2672, 0x1611, 0x0630, 0x76d7, 0x66f6, 0x5695, 0x46b4,
        0xb75b, 0xa77a, 0x9719, 0x8738, 0xf7df, 0xe7fe, 0xd79d, 0xc7bc,
        0x48c4, 0x58e5, 0x6886, 0x78a7, 0x0840, 0x1861, 0x2802, 0x3823,
        0xc9cc, 0xd9ed, 0xe98e, 0xf9af, 0x8948, 0x9969, 0xa90a, 0xb92b,
        0x5af5, 0x4ad4, 0x7ab7, 0x6a96, 0x1a71, 0x0a50, 0x3a33, 0x2a12,
        0xdbfd, 0xcbdc, 0xfbbf, 0xeb9e, 0x9b79, 0x8b58, 0xbb3b, 0xab1a,
        0x6ca6, 0x7c87, 0x4ce4, 0x5cc5, 0x2c22, 0x3c03, 0x0c60, 0x1c41,
        0xedae, 0xfd8f, 0xcdec, 0xddcd, 0xad2a, 0xbd0b, 0x8d68, 0x9d49,
        0x7e97, 0x6eb6, 0x5ed5, 0x4ef4, 0x3e13, 0x2e32, 0x1e51, 0x0e70,
        0xff9f, 0xefbe, 0xdfdd, 0xcffc, 0xbf1b, 0xaf3a, 0x9f59, 0x8f78,
        0x9188, 0x81a9, 0xb1ca, 0xa1eb, 0xd10c, 0xc12d, 0xf14e, 0xe16f,
        0x1080, 0x00a1, 0x30c2, 0x20e3, 0x5004, 0x4025, 0x7046, 0x6067,
        0x83b9, 0x9398, 0xa3fb, 0xb3da, 0xc33d, 0xd31c, 0xe37f, 0xf35e,
        0x02b1, 0x1290, 0x22f3, 0x32d2, 0x4235, 0x5214, 0x6277, 0x7256,
        0xb5ea, 0xa5cb, 0x95a8, 0x8589, 0xf56e, 0xe54f, 0xd52c, 0xc50d,
        0x34e2, 0x24c3, 0x14a0, 0x0481, 0x7466, 0x6447, 0x5424, 0x4405,
        0xa7db, 0xb7fa, 0x8799, 0x97b8, 0xe75f, 0xf77e, 0xc71d, 0xd73c,
        0x26d3, 0x36f2, 0x0691, 0x16b0, 0x6657, 0x7676, 0x4615, 0x5634,
        0xd94c, 0xc96d, 0xf90e, 0xe92f, 0x99c8, 0x89e9, 0xb98a, 0xa9ab,
        0x5844, 0x4865, 0x7806, 0x6827, 0x18c0, 0x08e1, 0x3882, 0x28a3,
        0xcb7d, 0xdb5c, 0xeb3f, 0xfb1e, 0x8bf9, 0x9bd8, 0xabbb, 0xbb9a,
        0x4a75, 0x5a54, 0x6a37, 0x7a16, 0x0af1, 0x1ad0, 0x2ab3, 0x3a92,
        0xfd2e, 0xed0f, 0xdd6c, 0xcd4d, 0xbdaa, 0xad8b, 0x9de8, 0x8dc9,
        0x7c26, 0x6c07, 0x5c64, 0x4c45, 0x3ca2, 0x2c83, 0x1ce0, 0x0cc1,
        0xef1f, 0xff3e, 0xcf5d, 0xdf7c, 0xaf9b, 0xbfba, 0x8fd9, 0x9ff8,
        0x6e17, 0x7e36, 0x4e55, 0x5e74, 0x2e93, 0x3eb2, 0x0ed1, 0x1ef0,
    ]

    def __init__(self,
                 log: logging.Logger,
                 port: str,
                 baudrate: int,
                 ser_timeout: float = 0.0):
        self.log = log
        self.ser = serial.Serial(port, baudrate, timeout=ser_timeout)
        self.header_pad = b'\x00'

    def calc_crc(self, data, crc=0):
        for char in bytearray(data):
            crctbl_idx = ((crc >> 8) ^ char) & 0xff
            crc = ((crc << 8) ^ self.crctable[crctbl_idx]) & 0xffff
        return crc & 0xffff

    def set_timeout(self, timeout):
        if timeout:
            self.ser.timeout = timeout
        pass

    def set_baudrate(self, baud):
        self.ser.baudrate = baud
        pass

    def hardware_reset(self):
        self.ser.dtr = 0
        self.ser.rts = 1
        time.sleep(0.3)
        self.ser.rts = 0
        pass

    def reset(self):
        self.ser.reset_input_buffer()
        self.ser.reset_output_buffer()

    def send_data(self, data: bytes):
        return self.ser.write(data)

    def read_data(self, length):
        data = self.ser.read(length)
        return data

    def read_line(self, times=1, timeout=0) -> bytes:
        record_timeout = self.ser.timeout
        self.set_timeout(timeout)
        data = b''
        for _ in range(times):
            data += self.ser.readline()
        self.set_timeout(record_timeout)
        return data

    def check_ram_mode(self, times=2, timeout=1):
        data = 'version\r\n'
        self.reset()
        self.send_data(data.encode("utf-8"))
        uart_rx_byte = self.read_line(times, timeout)
        self.log.debug(f"check_ram_mode receive: {uart_rx_byte}")
        if "RAMCODE".encode("utf-8") in uart_rx_byte:
            return True
        return False

    def get_flash_uuid(self):
        data = 'flash_uid\r\n'
        self.reset()
        self.send_data(data.encode("utf-8"))
        flash_uid = self.read_line(2, timeout=1)
        if len(flash_uid) >= 30:
            return flash_uid
        return False

    def flash_set_addr(self):
        data = "startaddr 0x0\r\n"
        retOK = "pppp\r\n".encode("utf-8")
        # retFail = "ffff\r\n".encode("utf-8")
        self.reset()
        self.send_data(data.encode("utf-8"))
        res = self.read_line(2, timeout=1)
        if retOK in res:
            return True
        return False

    def abort(self, count=2):
        for _ in range(count):
            self.send_data(CAN)

    def _make_send_header(self, packet_size, sequence):
        assert packet_size in (128, 1024, 16*1024), packet_size
        _bytes = []
        if packet_size == 128:
            _bytes.append(ord(SOH))
        elif packet_size <= 16*1024:
            _bytes.append(ord(STX))
        _bytes.extend([sequence, 0xff - sequence])
        return bytearray(_bytes)

    def _make_send_checksum(self, data):
        _bytes = []
        crc = self.calc_crc(data)
        _bytes.extend([crc >> 8, crc & 0xff])
        return bytearray(_bytes)

    def receive_header(self, retry=20):
        error_count = 0
        cancel_count = 0
        while True:
            char = self.read_data(1)
            self.log.debug(f"char: {char}")

            if char == CRC:
                self.log.debug(f"Receive CRC [{CRC}] success.")
                return True
            elif char == CAN:
                cancel_count += 1
                self.log.debug(f"Receive CAN [{CAN}].")
            else:
                error_count += 1
                self.log.warning(f"Receive unknow [{char}][{error_count}].")

            if cancel_count >= 2:
                self.log.error("Receive CAN twice.")
                return False
            if error_count > retry:
                self.abort()
                self.log.error(f"Expect CRC more than [{retry}] times.")
                return False
        pass

    def receive_response(self, file_name, file_size, retry=20):
        header = self._make_send_header(128, 0)
        name = bytes(file_name, encoding="utf8")
        size = bytes(str(file_size), encoding="utf8")
        data = name + b'\x00' + size + b'\x20'
        data = data.ljust(128, self.header_pad)
        checksum = self._make_send_checksum(data)
        data_for_send = header + data + checksum
        self.send_data(data_for_send)

        error_count = 0
        cancel_count = 0
        while True:
            char = self.read_data(1)
            if char == ACK:
                self.log.debug(f"Receive ACK [{ACK}] success.")
                char2 = self.read_data(1)
                if char2 == CRC:
                    self.log.debug(f"Receive CRC [{CRC}] success.")
                else:
                    self.log.warning(f"ACK wasn't CRCd [{char2}].")
                return True
            elif char == CAN:
                cancel_count += 1
                self.log.debug(f"Receive CAN [{CAN}].")
            else:
                error_count += 1
                self.log.warning(f"Receive unknow [{char}][{error_count}].")

            if cancel_count >= 2:
                self.log.error("Receive CAN twice.")
                return False
            if error_count > retry:
                self.abort()
                self.log.error(f"Expect ACK more than [{retry}] times.")
                return False
        pass

    def send_file(self, file_stream, packet_size, is_stop,
                  callback=lambda: None, retry=20):
        sequence = 1
        while True:
            if is_stop():
                return False
            data = file_stream.read(packet_size)
            if not data:
                self.log.debug("send at EOF.")
                return True
            header = self._make_send_header(packet_size, sequence)
            data = data.ljust(packet_size, self.header_pad)
            checksum = self._make_send_checksum(data)
            data_for_send = header + data + checksum

            self.log.debug(f"send file sequence [{sequence}]")
            self.send_data(data_for_send)
            error_count = 0
            while True:
                char = self.read_data(1)
                if char == ACK:
                    break
                error_count += 1
                if error_count > retry:
                    self.log.error(f"Expect ACK more than \
[{retry}] times.")
                    self.abort()
                    return False
            callback()
            sequence = (sequence + 1) % 0x100
        pass

    def send_eot(self, retry=20):
        error_count = 0
        while True:
            self.send_data(EOT)
            self.log.debug("Send EOT.")
            char = self.read_data(1)
            if char == ACK:
                self.log.debug(f"Send EOT [{EOT}] success.")
                break
            error_count += 1
            if error_count > retry:
                self.abort()
                self.log.error(f"Expect ACK more than \
[{retry}] times.")
                return False

        header = self._make_send_header(128, 0)
        data = bytearray(b'\x00')
        data = data.ljust(128, self.header_pad)
        checksum = self._make_send_checksum(data)
        data_for_send = header + data + checksum
        self.send_data(data_for_send)

        error_count = 0
        while True:
            char = self.read_data(1)
            if char == ACK:
                break
            error_count += 1
            if error_count > retry:
                self.abort()
                self.log.error(f"Expect ACK more than \
[{retry}] times.")
                return False

        return True

    def close(self):
        self.ser.close()
        pass


class LN882HFlashHandler(FlashHandler):
    def __init__(self,
                 argv: FlashArgv,
                 logger: logging.Logger = None,
                 progress: ProgressHandler = None):
        super().__init__(argv, logger, progress)
        self.ser_handle = LN882HFlashSerial(logger, argv.port, 115200, 10)
        self.chip_info = {"QS200": "Mar 14 2021/00:23:32\r\n"}
        pass

    def serial_close(self):
        self.ser_handle.close()
        pass

    def check_stop(self):
        return self.stop_flag

    def send(self, file_stream, file_name, file_size, packet_size=128):
        total_write = (file_size // packet_size) + 2
        self.progress.setup("Writing", total_write)
        self.progress.start()

        if not self.ser_handle.receive_header():
            self.progress.close()
            return False
        self.progress.update()
        if not self.ser_handle.receive_response(file_name, file_size):
            self.progress.close()
            return False
        self.progress.update()
        if not self.ser_handle.send_file(file_stream, packet_size,
                                         self.check_stop,
                                         callback=self.progress.update):
            file_stream.close()
            self.progress.close()
            return False
        file_stream.close()
        if not self.ser_handle.send_eot():
            self.progress.close()
            return False
        self.progress.close()
        return True

    def show_version(self):
        self.logger.info("Waiting Reset ...")
        data = 'version\r\n'
        chip_info = self.chip_info["QS200"]
        over_time = 20

        while over_time > 0:
            if self.stop_flag:
                return False
            # self.ser_handle.hardware_reset()
            self.ser_handle.reset()
            self.ser_handle.send_data(data.encode("utf-8"))
            uart_rx_byte = self.ser_handle.read_line(2, timeout=1)
            self.logger.debug(f"version receive: {uart_rx_byte}")
            if ("RAMCODE\r\n".encode("utf-8") in uart_rx_byte) \
                    or (chip_info.encode("utf-8") in uart_rx_byte):
                self.logger.info(f"Show version receive: {uart_rx_byte}")
                return True
            over_time -= 1

        return False

    def check_boot_version(self):
        self.logger.debug("check ram mode first ...")
        mode = self.ser_handle.check_ram_mode()
        if mode:
            self.logger.info("Check ram mode success.")
            return True

        self.logger.warning("check ram mode fail first.")
        ram_bin_stream = io.BytesIO(RAM_BIN)
        size = len(RAM_BIN)
        data = 'download [rambin] [0x20000000] [%d]\r\n' % (size)
        self.ser_handle.send_data(data.encode("utf-8"))
        self.logger.info("Downloading ram.bin ...")
        if self.send(ram_bin_stream, "ram.bin", size, 1024) is True:
            self.logger.info("Checking download ...")
            res = self.ser_handle.read_data(300)
            self.logger.debug(f"download ram.bin reveive: {res}")
            self.logger.info("ram download success.")

        if self.stop_flag:
            return False

        self.logger.debug("check ram mode again ...")
        mode = self.ser_handle.check_ram_mode()
        if mode:
            self.logger.info("Check ram mode success.")
            return True

        self.logger.error("check ram mode fail again.")
        return False

    def show_flash_uuid(self):
        self.logger.debug("Show flash uuid ...")
        flash_uuid = self.ser_handle.get_flash_uuid()
        if flash_uuid:
            self.logger.debug(f"flash_uuid: {flash_uuid}")
        else:
            self.logger.debug("get flash uuid fail.")
        pass

    def set_baudrate(self, baud, retry=1):
        self.logger.info(f"Setting baudrate [{baud}] ...")
        data = f"baudrate {baud}\r\n"
        try_cnt = 0
        while try_cnt < retry:
            if self.stop_flag:
                return False
            self.ser_handle.reset()
            self.ser_handle.send_data(data.encode("utf-8"))
            uart_rx_byte = self.ser_handle.read_line(2, timeout=1)
            self.logger.debug(f"uart_rx_byte: {uart_rx_byte}")

            self.ser_handle.set_baudrate(baud)
            time.sleep(1)

            self.logger.debug("check baudrate ...")
            mode = self.ser_handle.check_ram_mode(2, 1)
            if not mode:
                try_cnt += 1
                self.logger.warning(f"Try set baudrate [{try_cnt}].")
            else:
                break

        if try_cnt >= retry:
            self.logger.error(f"Set baudrate [{baud}] fail.")
            return False

        self.logger.info(f"Set baudrate [{baud}] success.")
        return True

    def shake(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        self.logger.info("Note: Reset before starting the tyutool.")

        if not self.show_version():
            return False

        if not self.check_boot_version():
            return False

        # self.show_flash_uuid()
        if not self.set_baudrate(self.baudrate, retry=3):
            return False

        return True

    def erase(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        self.logger.info("Erasing ... ")
        addr = 0
        data_len = 1228*1024
        data = "ferase "
        data = data + hex(addr) + " "
        data = data + hex(data_len) + "\r\n"
        retOK = "pppp\r\n".encode("utf-8")
        # retFail = "ffff\r\n"
        self.ser_handle.reset()
        self.ser_handle.send_data(data.encode("utf-8"))
        res = self.ser_handle.read_line(2, timeout=1)
        self.logger.debug(f"erase receive: {res}")
        if retOK not in res:
            self.logger.error("Erase flash Fail.")
            return False
        self.logger.info("Erase flash success")
        return True

    def write(self):
        self.logger.debug(sys._getframe().f_code.co_name)

        if not self.ser_handle.flash_set_addr():
            self.logger.error("Set flash addr fail.")
            return False

        self.logger.info("Downloading qio.bin ...")
        data = "upgrade\r\n"
        self.ser_handle.send_data(data.encode("utf-8"))
        self.ser_handle.read_data(100)
        self.ser_handle.reset()

        file_stream = open(self.binfile, 'rb')
        file_size = os.path.getsize(self.binfile)
        if not self.send(file_stream, "qio.bin", file_size, 16*1024):
            return False

        self.logger.info("Write flash success")
        return True

    def crc_check(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        '''
        do something
        '''
        self.logger.info("CRC check success")
        return True

    def reboot(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        data = "reboot\r\n"
        retOK = "pppp".encode("utf-8")
        self.ser_handle.reset()
        self.ser_handle.send_data(data.encode("utf-8"))
        res = self.ser_handle.read_line(2, timeout=1)
        self.logger.debug(f"reboot receive: {res}")
        if retOK not in res:
            self.logger.error("Reboot fail.")
            return False

        # self.ser_handle.hardware_reset()
        self.logger.info("Reboot done")
        return True

    def read(self, length):
        self.logger.debug(sys._getframe().f_code.co_name)
        self.logger.error("Don't support read.")
        return False
