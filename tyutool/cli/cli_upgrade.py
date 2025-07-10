#!/usr/bin/env python
# -*- coding: utf-8 -*-

import click

from tyutool.util import TyutoolUpgrade
from tyutool.util.util import get_logger


@click.command()
def cli():
    logger = get_logger()
    logger.debug("CLI upgrade")
    up_handle = TyutoolUpgrade(logger, "cli")
    up_handle.upgrade()
    pass
