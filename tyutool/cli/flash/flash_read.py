#!/usr/bin/env python
# -*- coding: utf-8 -*-

import click

from .progress import CliProgressHandler, CliProgressHandlerTqdm
from .choose_port import choose_port
from tyutool.flash import FlashArgv, FlashInterface, flash_params_check
from tyutool.util.util import get_logger


@click.command()
@click.option('-d', '--device',
              type=click.Choice(FlashInterface.get_soc_names(),
                                case_sensitive=False),
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
@click.option('-l', '--length',
              type=lambda x: int(x, 16), default="0x200000",
              help="Flash read length [0x200000]")
@click.option('-f', '--file', 'binfile',
              type=str, required=True,
              help="file of BIN")
@click.option('--tqdm', flag_value="tqdm",
              is_flag=True, default=False,
              help="Progress use tqdm")
def cli(device, port, baud, start, length, binfile, tqdm):
    logger = get_logger()
    logger.debug(f'device: {device}')
    logger.debug(f'port: {port}')
    logger.debug(f'baud: {baud}')
    logger.debug(f'start: {start}')
    logger.debug(f'length: {length}')
    logger.debug(f'file: {binfile}')

    if tqdm:
        progress = CliProgressHandlerTqdm()
    else:
        progress = CliProgressHandler()

    # use defaule param
    if not baud:
        baud = FlashInterface.get_baudrate(device)
        logger.info(f'Use default baudrate: [{baud}]')
    if not start:
        start = FlashInterface.get_start_addr(device)
        logger.info(f'Use default start addrrss: [{start:#04x}]')
    if not port:
        port = choose_port()

    # check params
    argv = FlashArgv("read", device, port, baud, start, binfile,
                     length=length)
    if not flash_params_check(argv, logger=logger):
        logger.error("Parameter check failure.")
        return False

    handler_obj = FlashInterface.get_flash_handler(device)
    soc_handler = handler_obj(argv,
                              logger=logger,
                              progress=progress)
    if soc_handler.shake() \
            and soc_handler.read(length):
        soc_handler.crc_check()
    soc_handler.reboot()
    soc_handler.serial_close()

    logger.info("Flash read done.")

    return True
