#!/usr/bin/env python3
# coding=utf-8

import os
import pprint
import threading
from datetime import datetime

from .web.protocol import TYPE_MAPPING
from .web.protocol import ProtocolParser
from .web.socket_connect import SocketConnector
from .web.show import DataDisplay
from .web.save_db import SaveDatabase


class WebAIDebugMonitor(object):
    def __init__(self,
                 host="localhost", port=5055, monitor_types=['a'],
                 save_dir="web_ai_debug", logger=None,
                 display_hook=None):
        now = datetime.now().strftime("%Y%m%d-%H%M%S")
        save_db = os.path.join(save_dir, now, "ai.db")

        self.host = host
        self.port = port
        self.monitor_types = monitor_types
        self.logger = logger

        self.connector = SocketConnector(host, port, logger)
        self.parser = ProtocolParser()
        self.save = SaveDatabase(save_db, logger)
        self.display = DataDisplay(logger, display_hook)
        pass

    def connect(self, timeout=10):
        if not self.connector.connect(timeout):
            return False

        self.connector.send_subscription(self.monitor_types)

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

                frag_flag = header['frag_flag']

                if frag_flag == 0:  # No fragmentation
                    # Parse packet
                    packet = self.parser.parse_packet(
                        header['payload'], header['signature'], header
                    )
                    self._handle_packet(packet)

                elif frag_flag == 1:  # Fragment start
                    self.fragment_buffer = [header['payload']]

                elif frag_flag == 2:  # Fragment middle
                    if self.fragment_buffer:
                        self.fragment_buffer.append(header['payload'])

                elif frag_flag == 3:  # Fragment end
                    if self.fragment_buffer:
                        self.fragment_buffer.append(header['payload'])
                        complete_payload = b''.join(self.fragment_buffer)
                        self.fragment_buffer = None

                        # Parse complete packet
                        packet = self.parser.parse_packet(
                            complete_payload, header['signature'], header
                        )
                        self._handle_packet(packet)
        pass

    def _handle_packet(self, packet):
        if not packet:
            return

        # sort_dicts=False: only for python3.8+
        packet_format = pprint.pformat(packet,
                                       indent=2,
                                       # sort_dicts=False,
                                       )
        self.logger.debug(f"packet: \n{packet_format}")

        should_monitor = False
        packet_type = packet['type']
        for monitor_type in self.monitor_types:
            if packet_type == TYPE_MAPPING.get(monitor_type):
                should_monitor = True
                break

        if should_monitor:
            self.save.save(packet)
            self.display.display_packet(packet)
        pass
