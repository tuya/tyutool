#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import logging

from .. import FlashArgv, ProgressHandler, FlashHandler


class XXXXXFlashHandler(FlashHandler):
    def __init__(self,
                 argv: FlashArgv,
                 logger: logging.Logger = None,
                 progress: ProgressHandler = None):
        super().__init__(argv, logger, progress)
        pass

    def shake(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        '''
        do something
        '''
        self.logger.info(f'sync baudrate {self.baudrate} success')

        return True

    def erase(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        '''
        do something
        '''
        self.logger.info("Erase flash success")
        return True

    def write(self):
        self.logger.debug(sys._getframe().f_code.co_name)
        '''
        do something
        '''
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
        '''
        do something
        '''
        self.logger.info("Reboot done")
        return True

    def read(self, length):
        self.logger.debug(sys._getframe().f_code.co_name)
        '''
        do something
        '''
        self.logger.info("Read flash success")
        return True
