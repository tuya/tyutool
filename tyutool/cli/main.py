#!/usr/bin/env python
# -*- coding: utf-8 -*-

import click
import logging

from tyutool.util import TyutoolUpgrade

from tyutool.cli.cli_upgrade import cli as upgrade_cli
from tyutool.cli.cli_monitor import cli as monitor_cli
from tyutool.cli.flash import flash_write_cli, flash_read_cli
from tyutool.util.util import set_clis, tyutool_version, set_logger


def ask_for_upgrade(server_version=""):
    # 0-yes 1-no* 2-skip
    print(f"Upgrade Tyutool to [{server_version}] ?")
    ans = input("Upgrade Tyutool [y(es) / n(o) / s(kip)]: ")
    if ans.lower() == "y":
        return 0
    elif ans.lower() == "s":
        return 2
    return 1


CLIS = {
    "write": flash_write_cli,
    "read": flash_read_cli,
    "monitor": monitor_cli,
    "upgrade": upgrade_cli,
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
    if not nocheck:
        up_handle = TyutoolUpgrade(logger, "cli")
        up_handle.ask_upgrade(ask_for_upgrade)
    logger.info("Run Tuya Uart Tool.")
    pass


if __name__ == '__main__':
    run()
    pass
