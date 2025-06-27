#!/usr/bin/env python
# -*- coding: utf-8 -*-

import struct
from abc import ABC, abstractmethod


class BaseBootRomProtocol(ABC):
    def __init__(self):
        self.base_tx_type_and_opcode = [0x01, 0xe0, 0xfc]
        self.rx_header_and_event = [0x04, 0x0e]

    def command_generate(self, cmd, payload=[]):
        command = bytearray()
        command.extend(self.base_tx_type_and_opcode)
        command.append(1 + len(payload))
        command.append(cmd)
        command.extend(payload)
        return command

    def rx_expect_length(self, payload_lenth):
        length = len(self.rx_header_and_event) \
                + 1 + len(self.base_tx_type_and_opcode) + 1 + payload_lenth
        return length

    def get_response_cmd(self, response_content):
        return response_content[6:7]

    def get_response_payload(self, response_content):
        return response_content[7:]

    def check_response_header_seg(self, response_content):
        return response_content.startswith(bytes(self.rx_header_and_event))

    def check_response_length_seg(self, response_content):
        return response_content[2] == len(response_content) - 3

    def check_response_tx_header_seg(self, response_content):
        return response_content[3:6] == bytes(self.base_tx_type_and_opcode)

    @abstractmethod
    def cmd(self):
        pass

    @abstractmethod
    def expect_length(self):
        pass

    def response_check(self, response_content):
        res = self.check_response_header_seg(response_content) \
            and self.check_response_length_seg(response_content) \
            and self.check_response_tx_header_seg(response_content)
        return res


class LinkCheckProtocol(BaseBootRomProtocol):
    def cmd(self):
        return self.command_generate(0x00)

    @property
    def expect_length(self):
        return self.rx_expect_length(1)

    def response_check(self, response_content):
        res = super().response_check(response_content) \
            and self.get_response_cmd(response_content) == bytes([0x01]) \
            and self.get_response_payload(response_content) == bytes([0x00])
        return res


class GetChipIdProtocol(BaseBootRomProtocol):
    def cmd(self, reg_addr):
        return self.command_generate(0x03,
                                     [reg_addr & 0xff,
                                      (reg_addr >> 8) & 0xff,
                                      (reg_addr >> 16) & 0xff,
                                      (reg_addr >> 24) & 0xff])

    @property
    def expect_length(self):
        return self.rx_expect_length(8)

    def response_check(self, response_content, reg_addr):
        if super().response_check(response_content) \
                and self.get_response_cmd(response_content) == bytes([0x03]):
            res_reg = response_content[7:11]
            reg_1 = bytes([reg_addr & 0xff,
                           (reg_addr >> 8) & 0xff,
                           (reg_addr >> 16) & 0xff,
                           (reg_addr >> 24) & 0xff])
            reg_2 = bytes([(reg_addr & 0xff) + 1,
                           (reg_addr >> 8) & 0xff,
                           (reg_addr >> 16) & 0xff,
                           (reg_addr >> 24) & 0xff])  # work around bootrom bug
            if res_reg == reg_1 or res_reg == reg_2:
                return True
        return False

    def get_chip_id(self, response_content):
        payload = self.get_response_payload(response_content)[4:8]
        return struct.unpack('<I', payload)[0]


class BaseBootRomFlashProtocol(ABC):
    def __init__(self):
        self.base_tx_header = [0x01, 0xe0, 0xfc, 0xff, 0xf4]
        self.base_rx_header = [0x04, 0x0e, 0xff, 0x01, 0xe0, 0xfc, 0xf4]
        self.STATUS_INFO = [
            {
                'code': 0x0,
                'desc': 'normal'
            },
            {
                'code': 0x1,
                'desc': 'FLASH_STATUS_BUSY'
            },
            {
                'code': 0x2,
                'desc': 'spi timeout'
            },
            {
                'code': 0x3,
                'desc': 'flash operate timeout'
            },
            {
                'code': 0x4,
                'desc': 'package payload lenth error'
            },
            {
                'code': 0x5,
                'desc': 'package lenth error'
            },
            {
                'code': 0x6,
                'desc': 'flash operate PARAM_ERROR'
            },
            {
                'code': 0x7,
                'desc': 'unkown cmd'
            },
        ]

    def command_generate(self, cmd, payload=[]):
        command = bytearray()
        command.extend(self.base_tx_header)
        command.extend([(1 + len(payload)) & 0xff,
                        ((1 + len(payload)) >> 8) & 0xff])
        command.append(cmd)
        command.extend(payload)
        return command

    def rx_expect_length(self, payload_lenth):
        # len operate  status
        return len(self.base_rx_header) + 2 + 1 + 1 + payload_lenth

    def get_response_payload(self, response_content):
        return response_content[11:]

    def get_response_cmd(self, response_content):
        # operate
        return response_content[9:10]

    def check_response_base_header(self, response_content):
        return response_content.startswith(bytes(self.base_rx_header))

    def check_response_status(self, response_content):
        status_code = response_content[10]
        if status_code == 0x0:
            return True
        else:
            for tmp_status in self.STATUS_INFO:
                if status_code == tmp_status['code']:
                    return False
            return False

    def check_response_length_seg(self, response_content):
        return response_content[7:9] == \
            bytes([(len(response_content) - 9) & 0xff,
                   ((len(response_content) - 9) >> 8) & 0xff])

    def response_check(self, response_content):
        res = self.check_response_base_header(response_content) \
            and self.check_response_length_seg(response_content) \
            and self.check_response_status(response_content)
        return res

    @abstractmethod
    def cmd(self):
        pass

    @abstractmethod
    def expect_length(self):
        pass


class GetFlashMidProtocol(BaseBootRomFlashProtocol):
    def cmd(self, reg_addr):
        return self.command_generate(0x0e, [reg_addr & 0xff, 0, 0, 0])

    @property
    def expect_length(self):
        return self.rx_expect_length(4)

    def check_response_length_seg(self, response_content):
        # fix bootrom bug
        len_res = response_content[7:9]
        len_1 = bytes([(len(response_content) - 9) & 0xff,
                       ((len(response_content) - 9) >> 8) & 0xff])
        len_2 = bytes([(len(response_content) - 9) & 0xff + 1,
                       ((len(response_content) - 9) >> 8) & 0xff])
        return (len_res == len_1) or (len_res == len_2)

    def response_check(self, response_content):
        return super().response_check(response_content) \
            and self.get_response_cmd(response_content) == bytes([0x0e])

    def get_mid(self, response_content):
        return struct.unpack("<I", response_content[11:])[0] >> 8


class SetBaudrateProtocol(BaseBootRomProtocol):
    def cmd(self, baudrate, delay_ms):
        return self.command_generate(0x0f, [baudrate & 0xff,
                                            (baudrate >> 8) & 0xff,
                                            (baudrate >> 16) & 0xff,
                                            (baudrate >> 24) & 0xff,
                                            delay_ms & 0xff])

    @property
    def expect_length(self):
        return self.rx_expect_length(5)

    def response_check(self, response_content, baudrate):
        res_check = super().response_check(response_content)
        res_payload = super().get_response_payload(response_content)[:4] == \
            bytes([baudrate & 0xff,
                   (baudrate >> 8) & 0xff,
                   (baudrate >> 16) & 0xff,
                   (baudrate >> 24) & 0xff])
        return res_check and res_payload


class FlashReadSRProtocol(BaseBootRomFlashProtocol):
    def cmd(self, reg_addr):
        return self.command_generate(0x0c, [reg_addr & 0xff])

    @property
    def expect_length(self):
        return self.rx_expect_length(2)

    def response_check(self, response_content, reg_addr):
        return super().response_check(response_content=response_content) \
            and response_content[11:12] == bytes([reg_addr])

    def get_status_regist_val(self, response_content):
        return response_content[12]


class FlashWriteSRProtocol(BaseBootRomFlashProtocol):
    def cmd(self, reg_addr, val):
        payload = [reg_addr]
        payload.extend(val)
        return self.command_generate(0x0d, payload)

    def expect_length(self, sr_size):
        return self.rx_expect_length(1 + sr_size)

    def response_check(self, response_content, reg_addr):
        return super().response_check(response_content) \
            and response_content[11:12] == bytes([reg_addr])


class FlashErase4kProtocol(BaseBootRomFlashProtocol):
    def cmd(self, addr):
        return self.command_generate(0x0b, [addr & 0xff,
                                            (addr >> 8) & 0xff,
                                            (addr >> 16) & 0xff,
                                            (addr >> 24) & 0xff])

    @property
    def expect_length(self):
        return self.rx_expect_length(4)

    def response_check(self, response_content, flash_addr):
        return super().response_check(response_content) \
            and response_content[11:15] == bytes([flash_addr & 0xff,
                                                  (flash_addr >> 8) & 0xff,
                                                  (flash_addr >> 16) & 0xff,
                                                  (flash_addr >> 24) & 0xff])


class FlashErase4kExtProtocol(BaseBootRomFlashProtocol):
    def cmd(self, addr):
        return self.command_generate(0xeb, [addr & 0xff,
                                            (addr >> 8) & 0xff,
                                            (addr >> 16) & 0xff,
                                            (addr >> 24) & 0xff])

    @property
    def expect_length(self):
        return self.rx_expect_length(4)

    def response_check(self):
        return True


class FlashCustomEraseProtocol(BaseBootRomFlashProtocol):
    def cmd(self, addr, size):
        return self.command_generate(0x0f, [size, addr & 0xff,
                                            (addr >> 8) & 0xff,
                                            (addr >> 16) & 0xff,
                                            (addr >> 24) & 0xff])

    @property
    def expect_length(self):
        return self.rx_expect_length(5)

    def response_check(self, response_content, size_cmd, flash_addr):
        return super().response_check(response_content) \
            and response_content[11:12] == bytes([size_cmd]) \
            and response_content[12:] == bytes([flash_addr & 0xff,
                                                (flash_addr >> 8) & 0xff,
                                                (flash_addr >> 16) & 0xff,
                                                (flash_addr >> 24) & 0xff])


class FlashRead4kProtocol(BaseBootRomFlashProtocol):
    def cmd(self, addr):
        return self.command_generate(0x09, [addr & 0xff,
                                            (addr >> 8) & 0xff,
                                            (addr >> 16) & 0xff,
                                            (addr >> 24) & 0xff])

    @property
    def expect_length(self):
        return self.rx_expect_length(4 + 4096)

    def response_check(self, response_content, flash_addr):
        return super().response_check(response_content) \
            and response_content[11:15] == bytes([flash_addr & 0xff,
                                                  (flash_addr >> 8) & 0xff,
                                                  (flash_addr >> 16) & 0xff,
                                                  (flash_addr >> 24) & 0xff])

    def get_read_content(self, response_content):
        return response_content[15:]


class FlashRead4kExtProtocol(BaseBootRomFlashProtocol):
    def cmd(self, addr):
        return self.command_generate(0xe9, [addr & 0xff,
                                            (addr >> 8) & 0xff,
                                            (addr >> 16) & 0xff,
                                            (addr >> 24) & 0xff])

    @property
    def expect_length(self):
        return self.rx_expect_length(4+4096)

    def response_check(self):
        return True


class FlashWrite4kProtocol(BaseBootRomFlashProtocol):
    def cmd(self, addr, data):
        payload = [addr & 0xff, (addr >> 8) & 0xff,
                   (addr >> 16) & 0xff, (addr >> 24) & 0xff]
        payload.extend(data)
        return self.command_generate(0x07, payload)

    @property
    def expect_length(self):
        return self.rx_expect_length(4)

    def response_check(self, response_content, flash_addr):
        return super().response_check(response_content=response_content) \
            and response_content[11:15] == bytes([flash_addr & 0xff,
                                                  (flash_addr >> 8) & 0xff,
                                                  (flash_addr >> 16) & 0xff,
                                                  (flash_addr >> 24) & 0xff])


class FlashWrite4kExtProtocol(BaseBootRomFlashProtocol):
    def cmd(self, addr, data):
        payload = [addr & 0xff, (addr >> 8) & 0xff,
                   (addr >> 16) & 0xff, (addr >> 24) & 0xff]
        payload.extend(data)
        return self.command_generate(0x0e7, payload)

    @property
    def expect_length(self):
        return self.rx_expect_length(4)

    def response_check(self, response_content):
        return True


class CheckCrcProtocol(BaseBootRomProtocol):
    def cmd(self, start_addr, end_addr):
        return self.command_generate(0x10, [start_addr & 0xff,
                                            (start_addr >> 8) & 0xff,
                                            (start_addr >> 16) & 0xff,
                                            (start_addr >> 24) & 0xff,
                                            end_addr & 0xff,
                                            (end_addr >> 8) & 0xff,
                                            (end_addr >> 16) & 0xff,
                                            (end_addr >> 24) & 0xff])

    @property
    def expect_length(self):
        return self.rx_expect_length(4)

    def response_check(self, response_content):
        return super().response_check(response_content)

    def get_crc_value(self, response_content):
        return response_content[7] + (response_content[8] << 8) + \
            (response_content[9] << 16) + (response_content[10] << 24)


class CheckCrcExtProtocol(BaseBootRomProtocol):
    def cmd(self, start_addr, end_addr):
        return self.command_generate(0x13, [start_addr & 0xff,
                                            (start_addr >> 8) & 0xff,
                                            (start_addr >> 16) & 0xff,
                                            (start_addr >> 24) & 0xff,
                                            end_addr & 0xff,
                                            (end_addr >> 8) & 0xff,
                                            (end_addr >> 16) & 0xff,
                                            (end_addr >> 24) & 0xff])

    @property
    def expect_length(self):
        return self.rx_expect_length(4)

    def response_check(self, response_content):
        return True


class RebootProtocol(BaseBootRomProtocol):
    def cmd(self):
        return self.command_generate(0x0e, [0xa5])

    @property
    def expect_length(self):
        raise Exception('no support')

    def response_check(self):
        return True
