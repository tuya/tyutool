#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import os
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
              type=int, default=115200,
              help="Uart baud rate")
@click.option('-s', '--save',
              type=str, default="ser_ai_debug",
              help="Save assets to catalog.")
def ser_cli(port, baud, save):
    logger = get_logger()
    logger.info(f"port: {port}")
    logger.info(f"baud: {baud}")
    logger.info(f"save: {save}")

    monitor = SerAIDebugMonitor(port, baud, save, logger)

    if not monitor.open_port():
        return

    # 启动读取线程
    monitor.start_reading()

    # 主命令循环
    print("\n支持命令:")
    print("start       - 启动录音")
    print("stop        - 停止录音")
    print("reset       - 重置录音")
    print("dump 0      - 转储参考通道到 dump_mic.pcm")
    print("dump 1      - 转储麦克风通道到 dump_ref.pcm")
    print("dump 2      - 转储AEC通道到 dump_aec.pcm")
    print("bg 0        - 5s white")
    print("bg 1        - 5s 1K-0dB (bg 1 1000)")
    print("bg 2        - 4s sweep frequency")
    print("volume 50   - 设置音量为 50%")
    print("quit        - 退出程序")

    # 检查并覆盖已存在的文件
    for channel in monitor.dump_files:
        filename = monitor.dump_files[channel]['name']
        if os.path.exists(filename):
            print(f"注意: 已存在文件 {filename}，将被覆盖")

    try:
        while True:
            user_input = input("> ").strip().lower()

            if user_input == 'quit':
                break

            # 处理dump命令
            elif user_input in ['dump 0', 'dump 1', 'dump 2']:
                channel = user_input.split()[1]
                monitor.start_dump(channel)

            # 处理其他命令
            elif user_input in ['start', 'stop', 'reset', 'bg 0', 'bg 1', 'bg 2']:
                monitor.send_command(user_input)

            # 处理 bg 1 单频率命令
            elif user_input.startswith('bg 1 '):
                monitor.send_command(user_input)

            # 处理音量设置
            elif user_input.startswith('volume '):
                monitor.send_command(user_input)

            # 提示支持命令
            else:
                print("支持的命令: start, stop, reset, dump 0, dump 1, dump 2, bg 0, bg 1, bg 2, quit")
    except KeyboardInterrupt:
        print("\n程序中断")
    finally:
        # 清理资源
        monitor.stop_reading()
        monitor.close_port()
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
