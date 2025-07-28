#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import click
import serial
import threading

from tyutool.util.util import get_logger
from tyutool.flash import FlashInterface

from .flash.choose_port import choose_port


@click.command()
@click.option('-d', '--device',
              type=click.Choice(FlashInterface.get_soc_names(),
                                case_sensitive=False),
              required=False,
              help="Soc name")
@click.option('-p', '--port',
              type=str, required=False,
              help="Target port")
@click.option('-b', '--baud',
              type=int,
              help="Uart baud rate")
@click.option('-s', '--save',
              type=str, default="monitor.log",
              help="Save log to file")
def cli(device, port, baud, save):
    logger = get_logger()
    logger.debug("CLI monitor")
    logger.debug(f'device in: {device}')
    logger.debug(f'port in: {port}')
    logger.debug(f'baud in: {baud}')
    if not port:
        port = choose_port()
    if device and baud is None:
        baud = FlashInterface.get_monitor_baudrate(device)
    if baud is None:
        baud = 115200
    logger.debug(f'use port: {port}')
    logger.debug(f'use baud: {baud}')

    try:
        ser = serial.Serial(port, baud, timeout=1)
    except serial.SerialException as e:
        logger.error(f"Open port failed: {e}")
        sys.exit(1)

    logger.info("Open Monitor. (Quit: Ctrl+c)")

    log_file = None
    if save:
        try:
            log_file = open(save, 'w', encoding='utf-8')
            logger.info(f"Saving log to: {save}")
        except Exception as e:
            logger.error(f"Failed to open log file: {e}")
            log_file = None

    stop_event = threading.Event()
    receive_thread = threading.Thread(target=receive_data,
                                      args=(ser, stop_event, logger, log_file))
    send_thread = threading.Thread(target=send_data,
                                   args=(ser, stop_event, logger))
    receive_thread.start()
    send_thread.start()

    try:
        while receive_thread.is_alive() and send_thread.is_alive():
            receive_thread.join(timeout=0.1)
            send_thread.join(timeout=0.1)
    except KeyboardInterrupt:
        logger.info('Press "Entry" ...')
        stop_event.set()

    receive_thread.join()
    send_thread.join()

    ser.close()

    if log_file:
        log_file.close()
        logger.info(f"Log saved to: {save}")

    logger.info("Monitor exit.")


def receive_data(ser, stop_event, logger, log_file=None):
    try:
        while not stop_event.is_set():
            if ser.in_waiting:
                data = ser.readline().decode('utf-8', errors='ignore').strip()
                if data:
                    print(data)
                    if log_file:
                        try:
                            log_file.write(data + '\n')
                            log_file.flush()
                        except Exception as e:
                            logger.error(f"Write log error: {e}")
    except Exception as e:
        logger.error(f"Recive error: {e}")


def send_data(ser, stop_event, logger):
    try:
        while not stop_event.is_set():
            input_data = sys.stdin.readline()
            ser.write(input_data.encode('utf-8'))
    except Exception as e:
        logger.error(f"Send error: {e}")
