#!/usr/bin/env python3
# coding=utf-8

from .esp32 import ESP32ROM


class ESP32C6ROM(ESP32ROM):
    CHIP_NAME = "ESP32-C6"

    SPI_REG_BASE = 0x60003000
    SPI_USR_OFFS = 0x18
    SPI_USR1_OFFS = 0x1C
    SPI_USR2_OFFS = 0x20
    SPI_MOSI_DLEN_OFFS = 0x24
    SPI_MISO_DLEN_OFFS = 0x28
    SPI_W0_OFFS = 0x58


class ESP32C6StubLoader(ESP32C6ROM):
    FLASH_WRITE_SIZE = 0x4000
    STATUS_BYTES_LENGTH = 2
    IS_STUB = True

    def __init__(self, rom_loader):
        self.logger = rom_loader.logger
        self.stub = rom_loader.stub
        # self.secure_download_mode = rom_loader.secure_download_mode
        self._port = rom_loader._port
        # self._trace_enabled = rom_loader._trace_enabled
        # self.cache = rom_loader.cache
        self.flush_input()  # resets _slip_reader


ESP32C6ROM.STUB_CLASS = ESP32C6StubLoader
