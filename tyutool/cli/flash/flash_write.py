#!/usr/bin/env python
# -*- coding: utf-8 -*-

import click

from .progress import CliProgressHandler, CliProgressHandlerTqdm
from .choose_port import choose_port
from tyutool.flash import FlashArgv, FlashInterface, flash_params_check
from tyutool.flash.gd32 import try_flash_gd32_device
from tyutool.util.util import get_logger


UNKNOWN_DEVICE_FLASH_HANDLERS = [
    ("GD32VW553H", try_flash_gd32_device),
]


def _normalize_device(ctx, param, value):
    if value is None:
        return value

    supported_map = {
        soc_name.upper(): soc_name for soc_name in FlashInterface.get_soc_names()
    }
    return supported_map.get(value.upper(), value)

def _handle_unknown_device(device, port, baud, start, binfile, progress, logger):
    device_upper = str(device).upper()
    for supported_device, try_flash_func in UNKNOWN_DEVICE_FLASH_HANDLERS:
        if device_upper == str(supported_device).upper():
            handled, result = try_flash_func(
                device=device,
                port=port,
                baud=baud,
                start=start,
                binfile=binfile,
                logger=logger,
            )
            if handled:
                return result

    logger.warning(f'Unknown device [{device}], fallback flow is used.')
    return False


@click.command()
@click.option('-d', '--device',
              type=str,
              callback=_normalize_device,
              required=True,
              help="Soc name")
@click.option('-p', '--port',
              type=str, required=False,
              help="Target port")
@click.option('-b', '--baud',
              type=int,
              help="Uart baud rate")
@click.option('-s', '--start',
              type=lambda x: int(x, 16),
              help="Flash address of start")
@click.option('-f', '--file', 'binfile',
              type=str, required=True, help="file of BIN")
@click.option('--tqdm', flag_value="tqdm",
              is_flag=True, default=False,
              help="Progress use tqdm")
def cli(device, port, baud, start, binfile, tqdm):
    logger = get_logger()
    logger.debug(f'device: {device}')
    logger.debug(f'port: {port}')
    logger.debug(f'baud: {baud}')
    logger.debug(f'start: {start}')
    logger.debug(f'file: {binfile}')

    if tqdm:
        progress = CliProgressHandlerTqdm()
    else:
        progress = CliProgressHandler()

    if not port:
        port = choose_port()

    handler_obj = FlashInterface.get_flash_handler(device)
    if not handler_obj:
        return _handle_unknown_device(device, port, baud, start, binfile,
                                      progress, logger)

    # use defaule param
    if not baud:
        baud = FlashInterface.get_baudrate(device)
        logger.info(f'Use default baudrate: [{baud}]')
    if not start:
        start = FlashInterface.get_start_addr(device)
        logger.info(f'Use default start address: [{start:#04x}]')

    # check params
    argv = FlashArgv("write", device, port, baud, start, binfile)
    if not flash_params_check(argv, logger=logger):
        logger.error("Parameter check failure.")
        return False

    soc_handler = handler_obj(argv,
                              logger=logger,
                              progress=progress)
    result = False
    if soc_handler.shake() \
            and soc_handler.erase() \
            and soc_handler.write():
        result = soc_handler.crc_check()

    soc_handler.reboot()
    soc_handler.serial_close()
    if result:
        logger.info("Flash write success.")
    else:
        logger.error("Flash write failed.")
    return True
