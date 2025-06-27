#!/usr/bin/env python
# -*- coding: utf-8 -*-

import logging


class FlashArgv(object):
    def __init__(self, mode,
                 device, port, baudrate,
                 start_addr, binfile,
                 length=None):
        self.mode = mode
        self.device = device
        self.port = port
        self.baudrate = baudrate
        self.start_addr = start_addr
        self.binfile = binfile
        self.length = length
        pass


class ProgressHandler(object):
    def __init__(self):
        pass

    def setup(self,
              header: str,
              total: int) -> None:
        pass

    def start(self) -> None:
        pass

    def update(self, size: int = 1) -> None:
        pass

    def close(self) -> None:
        pass


class FlashHandler(object):
    def __init__(self,
                 argv: FlashArgv,
                 logger: logging.Logger = None,
                 progress: ProgressHandler = None):

        self.mode = argv.mode
        self.device = argv.device
        self.binfile = argv.binfile
        self.port = argv.port
        self.baudrate = argv.baudrate
        self.start_addr = argv.start_addr
        self.logger = logger if logger else logging.getLogger()
        self.progress = progress if progress else ProgressHandler()
        self.stop_flag = False
        pass

    def serial_close(self):
        pass

    def shake(self) -> bool:
        return True

    def erase(self) -> bool:
        return True

    def write(self) -> bool:
        return True

    def crc_check(self) -> bool:
        return True

    def reboot(self) -> bool:
        return True

    def read(self, length: int) -> bool:
        return True

    def start(self):
        self.stop_flag = False
        pass

    def stop(self):
        self.stop_flag = True
        pass
