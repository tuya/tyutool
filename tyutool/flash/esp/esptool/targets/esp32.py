#!/usr/bin/env python3
# coding=utf-8

from ..loader import ESPLoader


class ESP32ROM(ESPLoader):
    CHIP_NAME = "ESP32"
    STATUS_BYTES_LENGTH = 4

    SPI_REG_BASE = 0x3FF42000
    SPI_USR_OFFS = 0x1C
    SPI_USR1_OFFS = 0x20
    SPI_USR2_OFFS = 0x24
    SPI_MOSI_DLEN_OFFS = 0x28
    SPI_MISO_DLEN_OFFS = 0x2C
    SPI_W0_OFFS = 0x80

    def get_erase_size(self, offset, size):
        return size


class ESP32StubLoader(ESP32ROM):
    FLASH_WRITE_SIZE = 0x4000  # matches MAX_WRITE_BLOCK in stub_loader.c
    STATUS_BYTES_LENGTH = 2  # same as ESP8266, different to ESP32 ROM
    IS_STUB = True

    def __init__(self, rom_loader):
        self.logger = rom_loader.logger
        self.stub = rom_loader.stub
        # self.secure_download_mode = rom_loader.secure_download_mode
        self._port = rom_loader._port
        # self._trace_enabled = rom_loader._trace_enabled
        # self.cache = rom_loader.cache
        self.flush_input()  # resets _slip_reader


ESP32ROM.STUB_CLASS = ESP32StubLoader
