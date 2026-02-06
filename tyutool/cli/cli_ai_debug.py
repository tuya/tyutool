#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import time
import click
import threading

from tyutool.ai_debug import TYPE_MAPPING
from tyutool.ai_debug import WebAIDebugMonitor
from tyutool.ai_debug import SerAIDebugMonitor
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
              type=str, default="web_ai_debug",
              help="Save db file to catalog")
def web_cli(ip, port, event, save):
    logger = get_logger()
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
@click.option('-p', '--port',
              type=str, required=False,
              help="Target port")
@click.option('-b', '--baud',
              type=int, default=460800,
              help="Uart baud rate")
@click.option('-s', '--save',
              type=str, default="ser_ai_debug",
              help="Save assets to catalog.")
def ser_cli(port, baud, save):
    logger = get_logger()
    logger.debug(f"port: {port}")
    logger.debug(f"baud: {baud}")
    logger.debug(f"save: {save}")

    monitor = SerAIDebugMonitor(port, baud, save, logger, gui_mode=False)

    if not monitor.open_port():
        return

    # 启动读取线程
    monitor.start_reading()

    # 主命令循环
    monitor.show_help()

    # Loop input cmd
    try:
        while True:
            user_input = input("> ").strip()

            if not monitor.process_input_cmd(user_input):
                break

    except KeyboardInterrupt:
        logger.info("\nKeyboard Interrupt")
    finally:
        # 清理资源
        monitor.stop_reading()
        monitor.close_port()
    pass


@click.command()
@click.option('-p', '--port',
              type=str, required=False,
              help="Target port")
@click.option('-b', '--baud',
              type=int, default=460800,
              help="Uart baud rate")
@click.option('-s', '--save',
              type=str, default="ser_ai_debug",
              help="Save assets to catalog.")
def ser_auto_cli(port, baud, save):
    logger = get_logger()
    logger.debug(f"port: {port}")
    logger.debug(f"baud: {baud}")
    logger.debug(f"save: {save}")

    monitor = SerAIDebugMonitor(port, baud, save, logger, gui_mode=False)

    if not monitor.open_port():
        return

    # 启动读取线程
    monitor.start_reading()

    # Loop input cmd
    try:
        monitor.auto_test()

    except KeyboardInterrupt:
        logger.info("\nKeyboard Interrupt")
    finally:
        # 清理资源
        monitor.stop_reading()
        monitor.close_port()
    pass


@click.command()
@click.option('-p', '--port',
              type=str, required=False,
              help="Target port")
@click.option('-b', '--baud',
              type=int, default=460800,
              help="Uart baud rate")
@click.option('-w', '--wait',
              type=int, default=5,
              help="wait for recvice uart dump.")
@click.option('-s', '--save',
              type=str, default="ser_ai_debug",
              help="Save assets to catalog.")
@click.argument('cmd_args', nargs=-1, required=True)

def ser_cli_cmd(port, baud, save, wait, cmd_args):
    logger = get_logger()
    logger.debug(f"port: {port}")
    logger.debug(f"baud: {baud}")
    logger.debug(f"save: {save}")
    logger.debug(f"wait: {wait}")
    logger.debug(f"cmd: {cmd_args}")

    monitor = SerAIDebugMonitor(port, baud, save, logger, gui_mode=False)

    if not monitor.open_port():
        return

    # 启动读取线程
    monitor.start_reading()

    # 输入命令，
    cmd = " ".join(cmd_args)
    print(cmd)
    monitor.process_input_cmd("".join(cmd))
    if cmd in ['dump 0', 'dump 1', 'dump 2', 'dump 3', 'dump 4']:
        time.sleep(wait)
    monitor.stop_reading()
    monitor.close_port()
    pass


CLIS = {
    "web": web_cli,
    "ser": ser_cli,
    "ser_cmd": ser_cli_cmd,
    "ser_auto": ser_auto_cli,
}


@click.command(cls=set_clis(CLIS),
               help="AI Debug Tools.",
               context_settings=dict(help_option_names=["-h", "--help"]))
def cli():
    pass
