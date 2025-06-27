#!/usr/bin/env python
# -*- coding: utf-8 -*-

import os
import serial
from serial.tools import list_ports

from . import FlashHandler
from .bk7231n.bk7231n_flash import BK7231NFlashHandler
#  from .bk7231nex.bkex_flash import BKEXFlashHandler
from .rtl8720cf.rtl8720cf_flash import RTL8720CFlashHandler
from .t5.t5_flash import T5FlashHandler
from .ln882h.ln882h_flash import LN882HFlashHandler
from .esp.esp_flash import ESPFlashHandler


class FlashInterface(object):
    '''
    Soc名称全部使用大写；
    handler必须存在，且可以使用；
    modules字段必须存在，但可以为空；
    pic尽量选择方形
    '''

    SocList = {
        "BK7231N": {
            "handler": BK7231NFlashHandler,
            "baudrate": 921600,
            "monitor_baudrate": 115200,
            "start_addr": 0x00,
            "modules": {
                "CBU": {
                    "url": "https://developer.tuya.com/en/docs/iot/cbu-module-datasheet?id=Ka07pykl5dk4u",
                    "pic": "https://images.tuyacn.com/fe-static/docs/img/d6784358-a206-4118-8e9c-80e6609c6802.png",
                },
            },
        },
        "BK7231X": {
            "handler": BK7231NFlashHandler,
            "baudrate": 921600,
            "monitor_baudrate": 115200,
            "start_addr": 0x00,
            "modules": {
                "CBU": {
                    "url": "https://developer.tuya.com/en/docs/iot/cbu-module-datasheet?id=Ka07pykl5dk4u",
                    "pic": "https://images.tuyacn.com/fe-static/docs/img/d6784358-a206-4118-8e9c-80e6609c6802.png",
                },
            },
        },
        "T2": {
            "handler": BK7231NFlashHandler,
            "baudrate": 921600,
            "monitor_baudrate": 115200,
            "start_addr": 0x00,
            "modules": {
                "T2-U": {
                    "url": "https://developer.tuya.com/en/docs/iot/T2-U-module-datasheet?id=Kce1tncb80ldq",
                    "pic": "https://images.tuyacn.com/fe-static/docs/img/4f9e8ad5-f820-4097-947a-0bdb10d87040.jpg",
                },
            },
        },
        "T3": {
            "handler": T5FlashHandler,
            "baudrate": 921600,
            "monitor_baudrate": 115200,
            "start_addr": 0x00,
            "modules": {
                "T3-U": {
                    "url": "https://developer.tuya.com/en/docs/iot/T3-U-Module-Datasheet?id=Kdd4pzscwf0il",
                    "pic": "https://images.tuyacn.com/fe-static/docs/img/5c5e71d4-f892-4e39-9eea-71e467bfafc9.jpg",
                },
            },
        },
        "T5": {
            "handler": T5FlashHandler,
            "baudrate": 921600,
            "monitor_baudrate": 460800,
            "start_addr": 0x00,
            "modules": {
                "T5-E1": {
                    "url": "https://developer.tuya.com/en/docs/iot/T5-E1-Module-Datasheet?id=Kdar6hf0kzmfi",
                    "pic": "https://images.tuyacn.com/fe-static/docs/img/47545473-ffb4-46dc-93e1-de672faa8b44.jpg",
                },
            },
        },
        "T5AI": {
            "handler": T5FlashHandler,
            "baudrate": 921600,
            "monitor_baudrate": 460800,
            "start_addr": 0x00,
            "modules": {
                "T5-E1": {
                    "url": "https://developer.tuya.com/en/docs/iot/T5-E1-Module-Datasheet?id=Kdar6hf0kzmfi",
                    "pic": "https://images.tuyacn.com/fe-static/docs/img/47545473-ffb4-46dc-93e1-de672faa8b44.jpg",
                },
            },
        },
        "RTL8720CF": {
            "handler": RTL8720CFlashHandler,
            "baudrate": 2000000,
            "monitor_baudrate": 115200,
            "start_addr": 0x00,
            "modules": {
                "WRB3": {
                    "url": "https://developer.tuya.com/cn/docs/iot/wbr3-module-datasheet?id=K9dujs2k5nriy",
                    "pic": "https://images.tuyacn.com/fe-static/docs/img/3c5b6f17-d17c-4897-a5ee-7d2808c2a153.jpg",
                },
            },
        },
        "RTL8720CM": {
            "handler": RTL8720CFlashHandler,
            "baudrate": 2000000,
            "monitor_baudrate": 115200,
            "start_addr": 0x00,
            "modules": {
                "CR3L": {
                    "url": "https://developer.tuya.com/cn/docs/iot/cr3l-module-datasheet?id=Ka3gl6ria8f1t",
                    "pic": "https://images.tuyacn.com/fe-static/docs/img/63a14d09-b4af-4c47-a7b1-25e1bda08b44.png",
                },
            },
        },
        "LN882H": {
            "handler": LN882HFlashHandler,
            "baudrate": 921600,
            "monitor_baudrate": 115200,
            "start_addr": 0x00,
            "modules": {
                "WL2H-U": {
                    "url": "https://developer.tuya.com/cn/docs/iot/WL2H-U-Module-Datasheet?id=Kbohlj8eg19u5",
                    "pic": "https://images.tuyacn.com/content-platform/hestia/16554364533921d740891.png",
                },
            },
        },
        "ESP32": {
            "handler": ESPFlashHandler,
            "baudrate": 921600,
            "monitor_baudrate": 115200,
            "start_addr": 0x00,
            "modules": {
                "ESP32-DevKitC-V4": {
                    "url": "https://docs.espressif.com/projects/esp-dev-kits/en/latest/esp32/esp32-devkitc/user_guide.html",
                    "pic": "https://docs.espressif.com/projects/esp-dev-kits/en/latest/esp32/_images/esp32-devkitc-v4-functional-overview.jpg",
                },
            },
        },
        "ESP32C3": {
            "handler": ESPFlashHandler,
            "baudrate": 921600,
            "monitor_baudrate": 115200,
            "start_addr": 0x00,
            "modules": {
                "ESP32-C3-DevKitM-1": {
                    "url": "https://docs.espressif.com/projects/esp-dev-kits/en/latest/esp32c3/esp32-c3-devkitm-1/user_guide.html",
                    "pic": "https://docs.espressif.com/projects/esp-dev-kits/en/latest/esp32c3/_images/esp32-c3-devkitm-1-v1-isometric.png",
                },
            },
        },
        "ESP32S3": {
            "handler": ESPFlashHandler,
            "baudrate": 921600,
            "monitor_baudrate": 115200,
            "start_addr": 0x00,
            "modules": {
                "ESP32-S3-DevKitC-1": {
                    "url": "https://docs.espressif.com/projects/esp-dev-kits/en/latest/esp32s3/esp32-s3-devkitc-1/user_guide.html",
                    "pic": "https://docs.espressif.com/projects/esp-dev-kits/en/latest/esp32s3/_images/esp32-s3-devkitc-1-v1.1-isometric.png",
                },
            },
        },
    }

    default_baudrate = 921600
    default_start_addr = 0x00

    @classmethod
    def get_soc_names(self):
        return self.SocList.keys()

    @classmethod
    def get_flash_handler(self, soc: str) -> FlashHandler:
        soc_name = soc.upper()
        if soc_name not in self.SocList.keys():
            return None
        handler = self.SocList[soc_name]['handler']
        return handler

    @classmethod
    def get_baudrate(self, soc: str) -> int:
        soc_name = soc.upper()
        if soc_name not in self.SocList.keys():
            return None
        soc_info = self.SocList[soc_name]
        if 'baudrate' not in soc_info.keys():
            return self.default_baudrate
        return soc_info['baudrate']

    @classmethod
    def get_monitor_baudrate(self, soc: str) -> int:
        soc_name = soc.upper()
        if soc_name not in self.SocList.keys():
            return 115200
        soc_info = self.SocList[soc_name]
        return soc_info.get("monitor_baudrate", 115200)

    @classmethod
    def get_start_addr(self, soc: str) -> int:
        soc_name = soc.upper()
        if soc_name not in self.SocList.keys():
            return None
        soc_info = self.SocList[soc_name]
        if 'start_addr' not in soc_info.keys():
            return self.default_baudrate
        return soc_info['start_addr']

    @classmethod
    def get_modules(self, soc: str) -> dict:
        soc_name = soc.upper()
        if soc_name not in self.SocList.keys():
            return {}
        modules = self.SocList[soc_name]['modules']
        return modules


def flash_params_check(argv, logger):
    mode = argv.mode
    device = argv.device
    binfile = argv.binfile
    start = argv.start_addr
    baud = argv.baudrate
    port = argv.port

    # check binile existx
    if mode.lower() == "write":
        if not os.path.exists(binfile):
            logger.error(f'File not exists: [{binfile}]')
            return False
    if mode.lower() == "read":
        if os.path.exists(binfile):
            logger.warning(f'File is exists: [{binfile}]')

    # check start addr
    if start & 0xfff:
        logger.warning(f'The start address [{start:#x}] is not aligned 4K')

    # check baudrat range
    baudrate_range = [115200, 230400, 460800, 921600, 1152000,
                      1500000, 2000000, 3000000, 4000000]
    if baud < 0 or baud > 2000000:
        logger.warning(f'Baudrate [{baud}] out of scope [0, 10000000]')
    if baud not in baudrate_range:
        logger.warning(f'Baudrate [{baud}] not in {baudrate_range}')

    # check port existx
    port_list = list(list_ports.comports())
    port_items = []
    for p in port_list:
        port_items.append(p.device)
    logger.debug(f'port_items: {port_items}')
    if port not in port_items:
        logger.error(f'Port [{port}] is not in port items.\n\
Port items: {port_items}')
        return False

    # check port busy
    try:
        serial.Serial(port, 9600, timeout=0.5)
    except Exception as e:
        logger.error(f'Exception: {e}')
        logger.error(f'Port [{port}] may be busy')
        return False

    # check device support
    handler_obj = FlashInterface.get_flash_handler(device)
    if not handler_obj:
        logger.error(f'Don\'t supported device: [{device}]')
        return False

    return True
