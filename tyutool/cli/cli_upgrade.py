#!/usr/bin/env python
# -*- coding: utf-8 -*-

import click

from tyutool.util import TyutoolUpgrade
from tyutool.util.util import get_logger


@click.command()
@click.option('-u', '--url',
              type=str, required=False,
              help="Configure file url")
def cli(url):
    logger = get_logger()
    logger.debug("CLI upgrade")
    logger.debug(f"url: {url}")
    up_handle = TyutoolUpgrade(logger, "cli", url)
    up_handle.upgrade()
    pass
