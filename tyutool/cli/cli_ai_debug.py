#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import click
import threading

from tyutool.ai_debug import TYPE_MAPPING
from tyutool.ai_debug import SocketConnector
from tyutool.ai_debug import ProtocolParser
from tyutool.util.util import get_logger


class AIDebugMonitor(object):
    def __init__(self,
                 host='localhost', port=5055, monitor_types=[],
                 logger=None):
        self.host = host
        self.port = port
        self.monitor_types = monitor_types
        self.logger = logger

        self.connector = SocketConnector(host, port, logger)
        pass

    def connect(self, timeout=10):
        if not self.connector.connect(timeout):
            return False

        self.connector.send_subscription(self.monitor_types)
        self.parser = ProtocolParser()

        self.running = True
        self.buffer = b''

        self.process_thread = threading.Thread(target=self._process_data_loop)
        self.process_thread.daemon = True
        self.process_thread.start()

        return True

    def disconnect(self):
        self.running = False
        if hasattr(self, 'process_thread') and self.process_thread.is_alive():
            self.process_thread.join(timeout=2)
        self.connector.disconnect()

    @property
    def connected(self):
        return self.connector.connected

    def _process_data_loop(self):
        while self.running and self.connector.connected:
            raw_data = self.connector.get_received_data(timeout=0.1)
            if not raw_data:
                continue
            self.buffer += raw_data

            while self.buffer:
                # Parse transport header using parser
                header, remaining = self.parser.parse_transport_header(
                    self.buffer
                )

                if header is None:
                    # Not enough data for a complete header
                    break

                self.buffer = remaining
                print(f"header: {header}")
        pass


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
def cli(ip, port, event):
    logger = get_logger()
    logger.debug("CLI debug")
    logger.info(f"ip: {ip}")
    logger.info(f"port: {port}")
    logger.info(f"event: {event}")

    monitor = AIDebugMonitor(ip, port, event, logger)

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
