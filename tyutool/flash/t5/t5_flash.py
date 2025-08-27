#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import logging
import serial
import time

from .. import FlashArgv, ProgressHandler, FlashHandler
from .protocol import (
    LinkCheckProtocol, GetChipIdProtocol,
    GetFlashMidProtocol, SetBaudrateProtocol,
    FlashReadSRProtocol, FlashWriteSRProtocol,
    FlashCustomEraseProtocol,
    FlashErase4kProtocol, FlashErase4kExtProtocol,
    FlashRead4kProtocol, FlashRead4kExtProtocol,
    FlashWrite4kProtocol, FlashWrite4kExtProtocol,
    CheckCrcProtocol, CheckCrcExtProtocol,
    RebootProtocol,
)
from .config.flash_config import FlashConfig


crc32_table = [0] * 256


def make_crc32_table():
    global crc32_table
    if crc32_table[255] != 0:
        return
    for i in range(256):
        c = i
        for bit in range(8):
            if c & 1:
                c = (c >> 1) ^ 0xEDB88320
            else:
                c = c >> 1
        crc32_table[i] = c


def crc32_ver2(crc, buf):
    make_crc32_table()
    for byte in buf:
        crc = (crc >> 8) ^ crc32_table[(crc ^ byte) & 0xFF]
    return crc


def is_buf_all_0xff(buf):
    length = len(buf)
    for i in range(length):
        if buf[i] != 0xff:
            return False
    return True


class T5FlashSerial(object):
    def __init__(self,
                 port: str,
                 baudrate: int,
                 ser_timeout: float = 0.0):
        self.ser = serial.Serial(port, baudrate, timeout=ser_timeout)
        pass

    def drain(self):
        self.ser.reset_input_buffer()

    def write_cmd(self, cmd):
        self.ser.write(cmd)

    def wait_for_cmd_response(self, expect_length, timeout_sec=0.1):
        timeout = serial.Timeout(timeout_sec)
        read_buf = b''
        while not timeout.expired():
            buf = self.ser.read(expect_length-len(read_buf))
            read_buf += buf
            if len(read_buf) == expect_length:
                break
        return read_buf

    def write_cmd_and_wait_response(self, cmd, expect_length, timeout_sec=0.1):
        self.drain()
        self.write_cmd(cmd)
        tmp_res = False
        ret_content = self.wait_for_cmd_response(expect_length=expect_length,
                                                 timeout_sec=timeout_sec)
        if len(ret_content) == expect_length:
            tmp_res = True

        return tmp_res, ret_content

    def do_reset(self):
        self.ser.dtr = 0
        self.ser.rts = 1
        time.sleep(0.3)
        self.ser.rts = 0
        pass

    def do_link_check(self, max_try_count=60):
        for _ in range(max_try_count):
            lcp = LinkCheckProtocol()
            res, content = self.write_cmd_and_wait_response(lcp.cmd(),
                                                            lcp.expect_length,
                                                            0.001)
            if res and lcp.response_check(content):
                return True
        return False

    def do_link_check_ex(self, max_try_count=60):
        cnt = max_try_count
        while cnt > 0:
            lcp = LinkCheckProtocol()
            res, content = self.write_cmd_and_wait_response(lcp.cmd(),
                                                            lcp.expect_length,
                                                            0.001)
            if res and lcp.response_check(content):
                return True
            cnt -= 1
        return False

    def get_bus(self, is_stop=None):
        max_try_count = 100
        for _ in range(max_try_count):
            if (is_stop is not None) and is_stop():
                return False
            self.do_reset()
            time.sleep(0.004)
            res = self.do_link_check_ex()
            if res:
                return True
        return False

    def get_chip_id(self):
        chip_id_reg_list = [0x44010004, 0x800000, 0x34010004]
        gcip = GetChipIdProtocol()
        self.drain()
        for tmp_reg in chip_id_reg_list:
            res, content = self.write_cmd_and_wait_response(gcip.cmd(tmp_reg),
                                                            gcip.expect_length,
                                                            0.5)
            if res and gcip.response_check(content, tmp_reg):
                tmp_chip_id = gcip.get_chip_id(content)
                if tmp_chip_id > 0 and tmp_chip_id != 0xffffffff:
                    return tmp_chip_id
        return None

    def set_baudrate(self, baudrate, delay_ms=20):
        if baudrate != self.ser.baudrate:
            self.drain()
            sbp = SetBaudrateProtocol()
            self.write_cmd(sbp.cmd(baudrate, delay_ms))
            time.sleep(delay_ms/2000)
            self.ser.baudrate = baudrate
            ret_content = self.wait_for_cmd_response(sbp.expect_length,
                                                     delay_ms / 1000 + 0.5)
            if len(ret_content) == sbp.expect_length:
                if sbp.response_check(ret_content, baudrate):
                    return True
            if self.do_link_check(5):
                return True
            else:
                return False
        else:
            return True

    def compare_register_value(self, src: list,
                               dest: list, mask: list):
        for _ in range(len(src)):
            if (src[_] & mask[_]) != (dest[_] & mask[_]):
                return False
        return True

    def _read_flash_status_reg_val(self, retry=5):
        frsp = FlashReadSRProtocol()
        read_reg_code = [5, 53]
        sr_val = []
        for _ in range(len(read_reg_code)):
            tmp_reg = read_reg_code[_]
            tmp_val = None
            for _ in range(retry):
                res, content = self.write_cmd_and_wait_response(
                    frsp.cmd(tmp_reg),
                    frsp.expect_length,
                    0.1)
                if res and frsp.response_check(content, tmp_reg):
                    tmp_val = frsp.get_status_regist_val(content)
                    break
                else:
                    continue
            if tmp_val is None:
                raise Exception('read flash status register fail')
            else:
                sr_val.append(tmp_val)
        return sr_val

    def _write_flash_status_reg_val(self, write_val, retry=5):
        fwsp = FlashWriteSRProtocol()
        write_reg_code = [1, 49]
        if len(write_reg_code) == 1:
            tmp_res = False
            for _ in range(retry):
                res, content = self.write_cmd_and_wait_response(
                    fwsp.cmd(write_reg_code[0], write_val),
                    fwsp.expect_length(len(write_val)),
                    0.1)
                if res and fwsp.response_check(content, write_reg_code[0]):
                    tmp_res = True
                    break
                else:
                    time.sleep(0.01)
                    continue

            if tmp_res is False:
                raise Exception('write flash status register fail')
        else:
            for idx in range(len(write_reg_code)):
                tmp_res = False
                for _ in range(retry):
                    res, content = self.write_cmd_and_wait_response(
                        fwsp.cmd(write_reg_code[idx],
                                 [write_val[idx]]),
                        fwsp.expect_length(1),
                        0.1)
                    if res and fwsp.response_check(content,
                                                   write_reg_code[idx]):
                        tmp_res = True
                        break
                    else:
                        time.sleep(0.01)
                        continue
                if tmp_res is False:
                    raise Exception('write flash status register fail')
                time.sleep(0.01)

    def disconnect(self):
        self.ser.close()

    def reset(self, baudrate=None):
        port = self.ser.port
        if baudrate is None:
            baudrate = self.ser.baudrate
        self.disconnect()
        self.ser = serial.Serial(port, baudrate, timeout=0)

    def unprotect_flash(self):
        unprotect_reg_val = [0, 0]
        mask = [124, 64]
        reg_val = self._read_flash_status_reg_val()
        if self.compare_register_value(reg_val, unprotect_reg_val, mask):
            return True
        else:
            write_val = unprotect_reg_val
            for _ in range(len(write_val)):
                write_val[_] = write_val[_] | (reg_val[_] & (mask[_] ^ 0xff))
            self._write_flash_status_reg_val(write_val)
            reg_val = self._read_flash_status_reg_val()
            if self.compare_register_value(reg_val, unprotect_reg_val, mask):
                return True
        return False

    def read_sector(self, flash_addr, flash_size):
        read_flash_protocol = FlashRead4kProtocol()
        if flash_size >= 256 * 1024 * 1024:
            read_flash_protocol = FlashRead4kExtProtocol()
        tmp_res, content = self.write_cmd_and_wait_response(
            read_flash_protocol.cmd(flash_addr),
            read_flash_protocol.expect_length,
            0.5)
        if tmp_res and read_flash_protocol.response_check(
                            content,
                            flash_addr=flash_addr):
            return read_flash_protocol.get_read_content(content)
        return None

    def read_and_check_sector(self, addr: int,
                              flash_size: int,
                              recnt=5):
        ret = None
        cnt = recnt
        while cnt > 0:
            ret = self.read_sector(addr, flash_size)

            if ret and self.check_crc_ver2(ret, addr, 0x1000,
                                           flash_size, recnt=recnt):
                return ret
            cnt -= 1
        return None

    def erase_sector(self, flash_addr, flash_size):
        erase_flash_protocol = FlashErase4kProtocol()
        if flash_size >= 256 * 1024 * 1024:
            erase_flash_protocol = FlashErase4kExtProtocol()
        tmp_res, content = self.write_cmd_and_wait_response(
                                    erase_flash_protocol.cmd(flash_addr),
                                    erase_flash_protocol.expect_length,
                                    0.5)
        if tmp_res and erase_flash_protocol.response_check(
                            content,
                            flash_addr=flash_addr):
            return True
        return False

    def erase_custom_size(self, flash_addr, cmd):
        '''
        cmd:
        normal,4k/32k/64k->0x20/0x52/0xd8
        ext,4k/32k/64k->0x21/0x5c/0xdc
        '''
        erase_flash_protocol = FlashCustomEraseProtocol()
        tmp_res, content = self.write_cmd_and_wait_response(
            erase_flash_protocol.cmd(flash_addr, cmd),
            erase_flash_protocol.expect_length,
            0.5)
        if tmp_res and erase_flash_protocol.response_check(
                                                content, cmd,
                                                flash_addr=flash_addr):
            return True
        return False

    def check_crc_ver2(self, buf: bytes,
                       flash_addr: int,
                       buf_len: int,
                       flash_size: int,
                       timeout=0.1, recnt=5):
        crc_protocol = CheckCrcProtocol()
        if flash_size >= 256 * 1024 * 1024:
            crc_protocol = CheckCrcExtProtocol()
        crc_me = crc32_ver2(0xffffffff, buf)
        for _ in range(recnt):
            crc_res, crc_content = self.write_cmd_and_wait_response(
                    crc_protocol.cmd(flash_addr, flash_addr+buf_len-1),
                    crc_protocol.expect_length,
                    timeout)
            if crc_res and crc_protocol.response_check(crc_content):
                break
        if not crc_res:
            return False
        crc_read = crc_protocol.get_crc_value(response_content=crc_content)
        if crc_me != crc_read:
            return False
        return True

    def write_sector(self, flash_addr, buf, flash_size):
        write_flash_protocol = FlashWrite4kProtocol()
        if flash_size >= 256 * 1024 * 1024:
            write_flash_protocol = FlashWrite4kExtProtocol()
        tmp_res, content = self.write_cmd_and_wait_response(
                        write_flash_protocol.cmd(flash_addr, buf),
                        write_flash_protocol.expect_length,
                        0.5)
        if tmp_res and write_flash_protocol.response_check(content,
                                                           flash_addr):
            return True
        return False

    def write_and_check_sector(self, buf_sec: bytes,
                               addr: int,
                               flash_size: int):
        length = len(buf_sec)
        if not self.write_sector(addr, buf_sec, flash_size):
            return False
        if not self.check_crc_ver2(buf_sec, addr, length, flash_size):
            return False
        return True

    def align_sector_address_for_write(self, addr: int,
                                       start_or_end: bool,
                                       content: bytes,
                                       flash_size: int):
        erase_addr = int(addr/0x1000)*0x1000
        baudrate_backup = self.ser.baudrate
        if not self.set_baudrate(115200):
            return False
        time.sleep(0.1)
        ret = self.read_sector(erase_addr, flash_size)
        if ret is None:
            return False
        if flash_size >= 256 * 1024 * 1024:
            res = self.erase_custom_size(erase_addr, 0x21)
        else:
            res = self.erase_custom_size(erase_addr, 0x20)
        if not res:
            return False
        if not self.set_baudrate(baudrate_backup):
            return False
        if start_or_end:
            ret = ret[:(addr & 0xfff)] + content[:(0x1000 - addr & 0xfff)]
        else:
            ret = content[-(addr & 0xfff):] + ret[(addr & 0xfff):]
        if not self.write_and_check_sector(ret, erase_addr, flash_size):
            return False
        return True

    def retry_write_sector(self, flash_addr: int,
                           buf: bytes,
                           flash_size: int,
                           recnt=5, is_stop=None):
        baudrate_backup = self.ser.baudrate
        self.reset(baudrate=115200)

        if self.get_bus(is_stop):
            return False
        time.sleep(0.01)
        if not self.set_baudrate(baudrate_backup):
            return False
        if not self.erase_sector(flash_addr, flash_size):
            return False
        if not self.write_and_check_sector(buf, flash_addr, flash_size):
            return False
        return True

    def close(self):
        self.ser.close()
        pass


class T5FlashHandler(FlashHandler):
    def __init__(self,
                 argv: FlashArgv,
                 logger: logging.Logger = None,
                 progress: ProgressHandler = None):
        super().__init__(argv, logger, progress)
        self.ser_handle = T5FlashSerial(argv.port, 115200, 0)  # init baudrate
        self.binfil = {}
        self.retry = 5
        self._flash_mid = None
        self._flash_cfg = FlashConfig()
        pass

    def serial_close(self):
        self.ser_handle.close()
        pass

    def check_stop(self):
        return self.stop_flag

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

    def shake(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        self.logger.info("Waiting Reset ...")
        res = self.ser_handle.get_bus(self.check_stop)
        if not res:
            self.logger.error("Get bus error.")
            return False

        time.sleep(0.05)  # fix read chip id fail sometimes
        chip_id = None
        cnt = self.retry
        while cnt > 0:
            if self.stop_flag:
                return False
            self.logger.debug("get chip id ...")
            chip_id = self.ser_handle.get_chip_id()
            if chip_id is not None:
                break
            cnt -= 1
        if chip_id is None:
            self.logger.error("Get chip id fail.")
            return False
        self.logger.debug("chip id is 0x{:x}".format(chip_id))

        self.logger.debug("get flash mid ...")
        fmp = GetFlashMidProtocol()
        res, content = self.ser_handle.write_cmd_and_wait_response(
                                        fmp.cmd(0x9f),
                                        fmp.expect_length,
                                        0.1)
        if not (res and fmp.response_check(content)):
            self.logger.error("Read flash mid fail..")
            return False
        self._flash_mid = fmp.get_mid(content)
        self.logger.debug(f'flash mid is 0x{self._flash_mid:x}')
        self._flash_cfg.parse_flash_info(self._flash_mid)

        self.logger.debug("sync baudrate ...")
        if not self.ser_handle.set_baudrate(baudrate=self.baudrate,
                                            delay_ms=20):
            self.logger.error(f'Sync baudrate {self.baudrate} fail.')
            return False
        self.logger.info("unprotect flash OK.")

        self.logger.info(f'sync baudrate {self.baudrate} success')
        return True

    def erase(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        if not self.ser_handle.unprotect_flash():
            self.logger.error("unprotect flash fail.")
            return False
        self.binfile_prepare()

        start_addr = self.start_addr
        end_addr = start_addr + self.binfil['len']
        self.logger.debug(f"erase start address: {start_addr:08x}")
        self.logger.debug(f"erase end address: {end_addr:08x}")
        if start_addr & 0xfff:
            start_addr = int((start_addr+0x1000)/0x1000)*0x1000

        if end_addr & 0xfff:
            end_addr = int(end_addr/0x1000)*0x1000

        erase_size = end_addr-start_addr
        progress_total = erase_size / 0x10000
        rem = erase_size % 0x10000
        progress_total += rem / 0x1000
        self.progress.setup("Erasing", progress_total)
        self.progress.start()
        self.progress.update()

        i = 0
        while i < erase_size:
            if self.stop_flag:
                self.progress.close()
                return False
            fmt_addr = f"{(start_addr+i):08x}"
            self.logger.debug(f"erase at {fmt_addr} ...")
            if erase_size-i > 0x10000:
                if (start_addr+i) & 0xffff:
                    cnt = self.retry
                    ret = False
                    while cnt > 0 and not ret:
                        if self._flash_cfg.flash_size >= 256 * 1024 * 1024:
                            ret = self.ser_handle.erase_custom_size(
                                start_addr+i, 0xdc)
                        else:
                            ret = self.ser_handle.erase_custom_size(
                                start_addr+i, 0xd8)
                        cnt -= 1
                    if not ret:
                        self.logger.error(f"Erase sector {fmt_addr} fail.")
                        return False
                    self.logger.debug("Erase size 0x1000(4K)")
                    i += 0x1000
                else:
                    cnt = self.retry
                    ret = False
                    while cnt > 0 and not ret:
                        if self._flash_cfg.flash_size >= 256 * 1024 * 1024:
                            ret = self.ser_handle.erase_custom_size(
                                start_addr+i, 0xdc)
                        else:
                            ret = self.ser_handle.erase_custom_size(
                                start_addr+i, 0xd8)
                        cnt -= 1
                    if not ret:
                        self.logger.error(
                            f"Erase block(64K) error: {fmt_addr}")
                        self.progress.close()
                        return False
                    self.progress.update()
                    self.logger.debug("Erase size 0x10000(64K)")
                    i += 0x10000
            else:
                cnt = self.retry
                ret = False
                while cnt > 0 and not ret:
                    if self._flash_cfg.flash_size >= 256 * 1024 * 1024:
                        ret = self.ser_handle.erase_custom_size(
                            start_addr+i, 0x21)
                    else:
                        ret = self.ser_handle.erase_custom_size(
                            start_addr+i, 0x20)
                    cnt -= 1
                if not ret:
                    self.logger.error(f"Erase sector(4K) error: {fmt_addr}")
                    self.progress.close()
                    return False
                self.progress.update()
                self.logger.debug("Erase size 0x1000(4K)")
                i += 0x1000
        time.sleep(0.1)  # 进度条显示完整

        self.progress.close()
        self.logger.info("Erase flash success")
        return True

    def write(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        start_addr = self.start_addr
        if start_addr & 0xfff:
            start_addr = int((start_addr + 0x1000)/0x1000)*0x1000

        wbuf = self.binfil['bin']
        file_len = self.binfil['len']
        end_addr = start_addr + file_len
        flash_size = self._flash_cfg.flash_size

        self.logger.debug(f"write flash {start_addr:08x}({file_len})")
        progress_total = 2 + (file_len / 0x1000) + 1
        self.progress.setup("Writing", progress_total)
        self.progress.start()

        if start_addr & 0xfff:
            self.logger.debug(f"write align start {start_addr:08x} ...")
            if not self.ser_handle.align_sector_address_for_write(
                                start_addr, True, wbuf,
                                flash_size):
                self.logger.error(f"Align start address \
{start_addr:08x} fail.")
                self.progress.close()
                return False
            wbuf = wbuf[(0x1000-start_addr & 0xfff):]
            start_addr = int((start_addr+0x1000)/0x1000)*0x1000
            file_len = len(wbuf)
        self.progress.update()

        if end_addr & 0xfff:
            self.logger.debug(f"write align end {end_addr:08x} ...")
            if not self.ser_handle.align_sector_address_for_write(
                                end_addr, False, wbuf,
                                flash_size):
                self.logger.error(f"Align end address {end_addr:08x} fail.")
                self.progress.close()
                return False
            wbuf = wbuf[:len(wbuf)-(end_addr & 0xfff)]
            end_addr = int(end_addr/0x1000)*0x1000
            file_len = len(wbuf)
        self.progress.update()

        i = 0
        while i < file_len:
            if self.stop_flag:
                self.progress.close()
                return False
            self.logger.debug(f"write at {(i+start_addr):08x} (4K) ...")
            if not is_buf_all_0xff(wbuf[i:i+0x1000]):
                if not self.ser_handle.write_and_check_sector(wbuf[i:i+0x1000],
                                                              i+start_addr,
                                                              flash_size):
                    self.logger.warning(f"Retry write at {(i+start_addr):08x}")
                    if not self.ser_handle.retry_write_sector(i+start_addr,
                                                              wbuf[i:i+0x1000],
                                                              flash_size,
                                                              self.retry,
                                                              self.check_stop):
                        self.logger.error(f"Error write at {(i+start_addr):08x}")
                        return False
            self.progress.update()
            i += 0x1000

        self.logger.debug("protect flash...")
        protect_reg_val, mask = self._flash_cfg.protect_register_value
        reg_val = self.ser_handle._read_flash_status_reg_val()
        if not self.ser_handle.compare_register_value(reg_val,
                                                      protect_reg_val,
                                                      mask):
            write_val = protect_reg_val
            for _ in range(len(write_val)):
                write_val[_] = write_val[_] | (reg_val[_] & (mask[_] ^ 0xff))
            self.ser_handle._write_flash_status_reg_val(write_val)
            reg_val = self.ser_handle._read_flash_status_reg_val()
            if not self.ser_handle.compare_register_value(reg_val,
                                                          protect_reg_val,
                                                          mask):
                self.logger.error("Protect flash fail.")
                self.progress.close()
                return False
        self.progress.update()
        time.sleep(0.1)  # 进度条显示完整

        self.progress.close()
        self.logger.info("Write flash success")
        return True

    def crc_check(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        '''
        do nothing
        '''
        self.logger.info("CRC check success")
        return True

    def reboot(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        rb_protocol = RebootProtocol()
        self.ser_handle.write_cmd(rb_protocol.cmd())

        self.logger.info("Reboot done")
        return True

    def read(self, length):
        self.logger.debug(sys._getframe().f_code.co_name)
        self.logger.debug("start read flash ...")

        flash_size = self._flash_cfg.flash_size
        start = self.start_addr
        file_buf = b''
        cnt = self.retry
        i = 0

        total_read = length // 0x1000
        self.progress.setup("Reading", total_read)
        self.progress.start()

        while i < length:
            if self.stop_flag:
                self.progress.close()
                return False
            self.logger.debug(f"read at {(start+i):08x} ...")
            ret = self.ser_handle.read_and_check_sector(start+i,
                                                        flash_size,
                                                        cnt)
            if ret is None:
                self.logger.error("Read flash fail.")
                return False
            self.progress.update()
            file_buf += ret
            i += 0x1000

        self.logger.debug(f"write bin file {self.binfile} ...")
        with open(self.binfile, 'wb') as f:
            f.write(file_buf[:length])
            f.close()

        self.progress.update()
        time.sleep(0.1)  # 进度条显示完整
        self.progress.close()

        self.binfile_prepare()
        self.logger.info("Read flash success")
        return True
