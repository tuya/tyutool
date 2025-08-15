#!/usr/bin/env python3
# coding=utf-8

import socket
import queue
import threading
import struct
import uuid
import logging


from .protocol import TYPE_MAPPING


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
        self.logger.debug("SocketConnector init.")
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
        # Construct Event packet attributes
        attributes = []

        # SessionID attribute (type 43)
        session_id = "debug-session"
        session_id_bytes = session_id.encode('utf-8')
        attributes.append({
            'type': 43,  # SessionID
            'payload_type': 6,  # string
            'length': len(session_id_bytes),
            'payload': session_id_bytes
        })

        # EventID attribute (type 61)
        event_id = str(uuid.uuid4())
        event_id_bytes = event_id.encode('utf-8')
        attributes.append({
            'type': 61,  # EventID
            'payload_type': 6,  # string
            'length': len(event_id_bytes),
            'payload': event_id_bytes
        })

        # UserData attribute (type 111) - contains bitmap data
        # Create 64-bit bitmap, each data type corresponds to one bit
        bitmap = 0
        # Set corresponding bit
        for monitor_type in monitor_types:
            packet_type = TYPE_MAPPING.get(monitor_type)
            if packet_type:
                bitmap |= (1 << packet_type)

        # Convert bitmap to 8-byte binary data
        bitmap_bytes = struct.pack('>Q', bitmap)
        attributes.append({
            'type': 111,  # UserData
            'payload_type': 5,  # bytes
            'length': len(bitmap_bytes),  # 固定8字节
            'payload': bitmap_bytes
        })

        # 构造attributes的二进制数据
        attributes_data = b''
        for attr in attributes:
            # 属性类型 (2字节)
            attributes_data += struct.pack('>H', attr['type'])
            # 属性内容类型 (1字节)
            attributes_data += struct.pack('B', attr['payload_type'])
            # 属性长度 (4字节)
            attributes_data += struct.pack('>I', attr['length'])
            # 属性内容
            attributes_data += attr['payload']

        # Construct Event packet data
        event_data = b''
        event_data += struct.pack('>H', 0xF000)  # MonitorTypeFilter event type
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

        # Construct transport layer protocol header
        transport_header = b''
        transport_header += struct.pack('>I', 0x54594149)  # magic "TYAI"
        transport_header += struct.pack('B', 0x02 << 6)    # direction
        transport_header += struct.pack('B', 0x01)          # version
        transport_header += struct.pack('>H', self.sequence)
        self.sequence = (self.sequence + 1) & 0xFFFF
        transport_header += struct.pack('B', 0x00)  # flags: no frag, L0, no IV
        transport_header += struct.pack('B', 0x00)  # reserve
        total_len = len(packet_data)
        transport_header += struct.pack('>I', total_len)

        # Send complete packet
        transport_packet = transport_header + packet_data

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
