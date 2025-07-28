#!/usr/bin/env python3
# coding=utf-8

import socket
import queue
import threading
import struct
import uuid
import logging


TYPE_MAPPING = {
    'a': 31,  # Audio
    't': 34,  # Text
    'v': 30,  # Video
    'p': 32,  # Image
}


class SocketConnector(object):
    def __init__(self, host="loacalhost", port=5055, logger=None):
        self.host = host
        self.port = port
        self.logger = logger or logging.getLogger()

        self.socket = None
        self.connected = False
        self.running = False
        self.sequence = 1
        self.data_queue = queue.Queue()
        self.receive_thread = None
        pass

    def _receive_loop(self):
        while self.running and self.socket:
            try:
                data = self.socket.recv(10 * 1024)
                if not data:
                    self.logger.warning("⚠️  Server disconnected.")
                    break
                self.data_queue.put(data)
            except Exception as e:
                if self.running:
                    self.logger.error(f"Receiving data failed: {e}")
                break
        self.connected = False
        pass

    def connect(self, timeout=10) -> bool:
        self.logger.info(f"Connecting socket: {self.host}:{self.port}.")

        try:
            self.socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            self.socket.settimeout(timeout)
            self.socket.connect((self.host, self.port))
        except socket.timeout:
            self.logger.error(f"❌ Timeout: {timeout}s.")
            return False
        except Exception as e:
            self.logger.error(f"❌ Connection failed: {str(e)}")
            return False

        # Cancel timeout limit after successful connection
        self.socket.settimeout(None)
        self.connected = True
        self.running = True

        # Start receive thread
        self.receive_thread = threading.Thread(target=self._receive_loop)
        self.receive_thread.daemon = True
        self.receive_thread.start()

        self.logger.info(f"✅ Connected to {self.host}:{self.port}")
        return True

    def send_subscription(self, monitor_types):
        # Create 64-bit bitmap, each data type corresponds to one bit
        bitmap = 0

        # Set corresponding bit
        for monitor_type in monitor_types:
            packet_type = TYPE_MAPPING.get(monitor_type)
            if packet_type:
                bitmap |= (1 << packet_type)

        # Convert bitmap to 8-byte binary data
        bitmap_bytes = struct.pack('>Q', bitmap)

        # Construct Event packet attributes
        attributes_data = b''

        # SessionID attribute (type 43)
        session_id = "monitor-session"
        session_id_bytes = session_id.encode('utf-8')
        attributes_data += struct.pack('>H', 43)  # attribute type
        attributes_data += struct.pack('B', 6)     # payload_type: string
        attributes_data += struct.pack('>I', len(session_id_bytes))
        attributes_data += session_id_bytes

        # EventID attribute (type 61)
        event_id = str(uuid.uuid4())
        event_id_bytes = event_id.encode('utf-8')
        attributes_data += struct.pack('>H', 61)  # attribute type
        attributes_data += struct.pack('B', 6)     # payload_type: string
        attributes_data += struct.pack('>I', len(event_id_bytes))
        attributes_data += event_id_bytes

        # UserData attribute (type 111) - contains bitmap data
        attributes_data += struct.pack('>H', 111)  # attribute type
        attributes_data += struct.pack('B', 5)      # payload_type: bytes
        attributes_data += struct.pack('>I', len(bitmap_bytes))
        attributes_data += bitmap_bytes

        # Construct Event packet data
        event_data = b''
        event_data += struct.pack('>H', 0x0009)  # MonitorTypeFilter event type
        event_data += struct.pack('>H', 0)       # Event payload length (0)

        # Construct Packet data
        packet_data = b''
        packet_type = 35  # Event type
        attribute_flag = 1  # Contains attributes
        type_and_flag = (packet_type << 1) | attribute_flag
        packet_data += struct.pack('B', type_and_flag)
        packet_data += struct.pack('>I', len(attributes_data))
        packet_data += attributes_data
        packet_data += struct.pack('>I', len(event_data))
        packet_data += event_data

        # Generate signature (simplified version, using all zeros)
        signature = b'\x00' * 32

        # Construct transport layer protocol header
        transport_header = b''
        transport_header += struct.pack('>I', 0x54594149)  # magic "TYAI"
        transport_header += struct.pack('B', 0x02 << 6)    # direction
        transport_header += struct.pack('B', 0x01)          # version
        transport_header += struct.pack('>H', self.sequence)
        self.sequence = (self.sequence + 1) & 0xFFFF
        transport_header += struct.pack('B', 0x00)  # flags: no frag, L0, no IV
        transport_header += struct.pack('B', 0x00)  # reserve
        total_len = len(packet_data) + len(signature)
        transport_header += struct.pack('>I', total_len)

        # Send complete packet
        transport_packet = transport_header + packet_data + signature

        try:
            self.socket.send(transport_packet)
        except Exception as e:
            self.logger.error(f"Failed to send subscription message: {e}")
            return False

        self.logger.info(f"📤 Subscription bitmap: 0x{bitmap:016X}")
        self.logger.info(f"📊 Monitoring data types: {monitor_types}")
        return True

    def disconnect(self):
        self.running = False
        self.connected = False

        if self.socket:
            try:
                self.socket.shutdown(socket.SHUT_RDWR)
                self.socket.close()
            except Exception as e:
                self.logger.waining(f"Socket disconnect failed: {e}")
                pass
            self.socket = None

        # Wait for receive thread to finish
        if self.receive_thread and self.receive_thread.is_alive():
            self.receive_thread.join(timeout=1)

        self.logger.info("🔌 Disconnected")

    def get_received_data(self, timeout=0.1):
        try:
            return self.data_queue.get(timeout=timeout)
        except queue.Empty:
            return None
