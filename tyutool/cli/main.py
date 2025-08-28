#!/usr/bin/env python
# -*- coding: utf-8 -*-

import click
import logging

from tyutool.cli.cli_monitor import cli as monitor_cli
from tyutool.cli.flash import flash_write_cli, flash_read_cli
from tyutool.util.util import set_clis, tyutool_version, set_logger


CLIS = {
    "write": flash_write_cli,
    "read": flash_read_cli,
    "monitor": monitor_cli,
}


@click.command(cls=set_clis(CLIS),
               help="Tuya Uart Tool.",
               context_settings=dict(help_option_names=["-h", "--help"]))
@click.option('-d', '--debug',
              is_flag=True, default=False,
              help="Show debug message")
@click.option('-n', '--nocheck',
              is_flag=True, default=False,
              help="No check upgrade")
@click.version_option(tyutool_version(), '-v', '--version',
                      prog_name="tyuTool")
def run(debug, nocheck):
    log_level = logging.INFO
    if debug:
        log_level = logging.DEBUG
    logger = set_logger(log_level)
    logger.info("Run Tuya Uart Tool.")
    pass


if __name__ == '__main__':
    run()
    pass
