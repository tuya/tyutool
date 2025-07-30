#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import click
import threading

from tyutool.ai_debug import TYPE_MAPPING
from tyutool.ai_debug import WebAIDebugMonitor
from tyutool.util.util import set_clis, get_logger


e_option_type = click.Choice(TYPE_MAPPING.keys(), case_sensitive=False)


@click.command()
@click.option('-i', '--ip',
              type=str, default="localhost",
              help="Server IP address")
@click.option('-p', '--port',
              type=int, default=5055,
              help="Target port")
@click.option('-e', '--event',
              type=e_option_type, default=['t'],
              multiple=True,
              help="Data type to monitor, can be specified multiple times")
@click.option('-s', '--save',
              type=str, default="ai_debug_db",
              help="Save db file to catalog")
def web_cli(ip, port, event, save):
    logger = get_logger()
    logger.debug("CLI debug")
    logger.info(f"ip: {ip}")
    logger.info(f"port: {port}")
    logger.info(f"event: {event}")
    logger.info(f"save: {save}")

    monitor = WebAIDebugMonitor(ip, port, event, save, logger)

    # Connect to server
    if not monitor.connect():
        sys.exit(1)

    try:
        logger.info("Press Ctrl+C to exit monitoring...")
        # Keep running
        while monitor.connected:
            threading.Event().wait(1)
    except KeyboardInterrupt:
        logger.info("\n⏹️  Stopping monitoring...")
    finally:
        monitor.disconnect()
    pass


@click.command()
def ser_cli(ip, port, event, save):
    pass


CLIS = {
    "web": web_cli,
    "ser": ser_cli,
}


@click.command(cls=set_clis(CLIS),
               help="AI Debug Tools.",
               context_settings=dict(help_option_names=["-h", "--help"]))
def cli():
    pass
