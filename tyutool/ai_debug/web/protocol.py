#!/usr/bin/env python3
# coding=utf-8

import struct

from tyutool.util.util import get_logger

TYPE_MAPPING = {
    'a': 31,  # Audio
    't': 34,  # Text
    'v': 30,  # Video
    'p': 32,  # Image
}


MEDIA_STREAM_COUNT = 0
MEDIA_STREAM_ID = ""


def _get_logger():
    """延迟初始化logger"""
    return get_logger()


class ProtocolParser:
    # Packet type constants
    PACKET_TYPES = {
        4: "Ping",
        5: "Pong",
        30: "Video",
        31: "Audio",
        32: "Image",
        33: "File",
        34: "Text",
        35: "Event"
    }

    # Attribute type constants
    ATTRIBUTE_TYPES = {
        61: "EventID",
        62: "EventTimestamp",
        63: "StreamStartTimestamp",
        71: "VideoCodecType",
        72: "VideoSampleRate",
        73: "VideoWidth",
        74: "VideoHeight",
        75: "VideoFPS",
        81: "AudioCodecType",
        82: "AudioSampleRate",
        83: "AudioChannels",
        84: "AudioBitDepth",
        91: "ImageFormat",
        92: "ImageWidth",
        93: "ImageHeight",
        101: "FileFormat",
        102: "FileName",
        111: "UserData",
        112: "SessionIDList",
        113: "ClientTimestamp",
        114: "ServerTimestamp"
    }

    STREAM_STATUS = {
        0: "Single packet",
        1: "Stream start",
        2: "Stream continue",
        3: "Stream end",
    }

    @staticmethod
    def _find_frame_sync(data):
        """Frame sync - Find magic field in data stream"""
        expected_magic = 0x54594149  # "TYAI"

        for i in range(1, len(data) - 3):
            if len(data) >= i + 4:
                magic = struct.unpack('>I', data[i:i+4])[0]
                if magic == expected_magic:
                    _get_logger().debug(f"Found magic field at offset {i}.")
                    return ProtocolParser.parse_transport_header(data[i:])

        _get_logger().debug("Magic field not found.")
        return None, data

    @staticmethod
    def parse_transport_header(data):
        """Parse transport layer protocol header"""
        if len(data) < 14:
            return None, data

        # Parse magic field
        magic = struct.unpack('>I', data[0:4])[0]
        expected_magic = 0x54594149  # "TYAI"
        if magic != expected_magic:
            _get_logger().debug("Magic field mismatch:")
            _get_logger().debug(f"expected 0x{expected_magic:08x}, \
actual 0x{magic:08x}")
            return ProtocolParser._find_frame_sync(data)

        # 0:device->cloud，1:cloud->device，2:device->debuger
        direction = (data[4] & 0xC0) >> 6  # 0x11000000

        version = data[5]
        if version != 1:
            _get_logger().debug(f"Unsupported version: {version}")
            return ProtocolParser._find_frame_sync(data[4:])

        sequence = struct.unpack('>H', data[6:8])[0]

        byte7 = data[8]
        frag_flag = (byte7 & 0xC0) >> 6  # 0x11000000
        security_level = (byte7 & 0x3E) >> 1  # 0x00111110 must 0
        iv_flag = byte7 & 0x01  # 0x00000001

        if security_level != 0 or iv_flag != 0:
            _get_logger().debug(f"Unsupported security level: \
{security_level}")
            return ProtocolParser._find_frame_sync(data[4:])

        reserve = data[9]

        header = {
            'magic': magic,
            'direction': direction,
            'version': version,
            'sequence': sequence,
            'frag_flag': frag_flag,
            'security_level': security_level,
            'iv_flag': iv_flag,
            'reserve': reserve
        }

        # iv: security_level=0 -> iv=none
        offset = 10

        # Parse length field
        if len(data) < offset + 4:
            return None, data

        length = struct.unpack('>I', data[offset:(offset+4)])[0]
        header['length'] = length
        offset += 4

        # Check if there's enough data
        total_needed = offset + length
        if len(data) < total_needed:
            return None, data

        # Extract payload and signature
        signature_length = 32
        payload_length = length - signature_length
        if payload_length < 0:
            _get_logger().error(f"Invalid payload length: {payload_length}")
            return None, data

        header['payload'] = data[offset:(offset+payload_length)]
        offset += payload_length

        header['signature'] = data[offset:(offset+signature_length)]

        return header, data[total_needed:]

    @staticmethod
    def parse_attributes(attr_data):
        """Parse attribute data"""
        attributes = {}
        offset = 0

        while offset < len(attr_data):
            if len(attr_data) < offset + 7:
                break

            attr_type = struct.unpack(
                '>H', attr_data[offset:offset + 2]
            )[0]
            payload_type = attr_data[offset + 2]
            attr_length = struct.unpack(
                '>I', attr_data[(offset+3):(offset+7)]
            )[0]

            offset += 7
            if len(attr_data) < offset + attr_length:
                break

            attr_payload = attr_data[offset:(offset+attr_length)]
            offset += attr_length

            # Parse attribute value based on payload_type
            if payload_type == 0x01 and len(attr_payload) >= 1:  # uint8
                value = attr_payload[0]
            elif payload_type == 0x02 and len(attr_payload) >= 2:  # uint16
                value = struct.unpack('>H', attr_payload)[0]
            elif payload_type == 0x03 and len(attr_payload) >= 4:  # uint32
                value = struct.unpack('>I', attr_payload)[0]
            elif payload_type == 0x04 and len(attr_payload) >= 8:  # uint64
                value = struct.unpack('>Q', attr_payload)[0]
            elif payload_type == 0x05:  # bytes
                value = attr_payload
            elif payload_type == 0x06:  # string
                value = attr_payload.decode('utf-8', errors='ignore')
            else:
                value = attr_payload

            attr_name = ProtocolParser.ATTRIBUTE_TYPES.get(
                attr_type, f"Attr{attr_type}"
            )
            attributes[attr_name] = value

        return attributes

    @staticmethod
    def parse_media_packet(payload):
        """Parse video/audio packet"""
        if len(payload) < 22:
            return {}

        global MEDIA_STREAM_COUNT
        global MEDIA_STREAM_ID

        data_id = struct.unpack('>H', payload[0:2])[0]
        byte2 = payload[2]
        stream_flag = (byte2 & 0xC0) >> 6  # 0x11000000
        status = ProtocolParser.STREAM_STATUS.get(stream_flag, "Unknown")
        timestamp = struct.unpack('>Q', payload[3:11])[0]
        pts = struct.unpack('>Q', payload[11:19])[0]
        length = struct.unpack('>I', payload[19:23])[0]

        media_payload = b''
        if len(payload) >= 23+length:
            media_payload = payload[23:(23+length)]

        # 针对流开始的包，更新流唯一标识，方便后续保存文件
        if stream_flag == 1:
            MEDIA_STREAM_COUNT += 1
            MEDIA_STREAM_ID = f"{MEDIA_STREAM_COUNT}_date{data_id}"

        return {
            'data_id': data_id,
            'stream_flag': stream_flag,
            'stream_status': status,
            'timestamp': timestamp,
            'pts': pts,
            'media_payload': media_payload,
            'size': length,
            'stream_id': MEDIA_STREAM_ID,  # 工具自定义字段
        }

    @staticmethod
    def parse_image_packet(payload):
        """Parse image packet"""
        if len(payload) < 15:
            return {}

        data_id = struct.unpack('>H', payload[0:2])[0]
        byte2 = payload[2]
        stream_flag = (byte2 & 0xC0) >> 6  # 0x11000000
        status = ProtocolParser.STREAM_STATUS.get(stream_flag, "Unknown")
        timestamp = struct.unpack('>Q', payload[3:11])[0]
        length = struct.unpack('>I', payload[11:15])[0]

        image_payload = b''
        if len(payload) >= 15+length:
            image_payload = payload[15:(15+length)]

        return {
            'data_id': data_id,
            'stream_flag': stream_flag,
            'stream_status': status,
            'timestamp': timestamp,
            'image_payload': image_payload,
            'size': length,
        }

    @staticmethod
    def parse_text_packet(payload):
        """Parse text packet"""
        if len(payload) < 7:
            return {}

        data_id = struct.unpack('>H', payload[0:2])[0]
        byte2 = payload[2]
        stream_flag = (byte2 >> 6) & 0x03
        status = ProtocolParser.STREAM_STATUS.get(stream_flag, "Unknown")
        length = struct.unpack('>I', payload[3:7])[0]

        text_data = b''
        if len(payload) >= 7+length:
            text_data = payload[7:(7+length)]
        text_content = text_data.decode('utf-8', errors='ignore')

        return {
            'data_id': data_id,
            'stream_flag': stream_flag,
            'stream_status': status,
            'text_content': text_content,
            'size': length,
        }

    @staticmethod
    def parse_packet(payload, signature, header):
        """Parse application layer packet"""
        if len(payload) < 5:
            return None

        if len(signature) != 32:
            return None

        # Parse first byte
        byte0 = payload[0]
        packet_type = (byte0 & 0xFE) >> 1  # 0x1111110
        attribute_flag = byte0 & 0x01  # 0x0000001
        type_name = ProtocolParser.PACKET_TYPES.get(
            packet_type, f"Unknown({packet_type})"
        )

        offset = 1
        attributes = {}

        # Parse attributes: 0-no attributes; 1-have attributes;
        if attribute_flag:
            if len(payload) < offset + 4:
                return None

            attr_length = struct.unpack('>I', payload[offset:(offset+4)])[0]
            offset += 4

            if len(payload) < offset + attr_length:
                return None

            attributes = ProtocolParser.parse_attributes(
                payload[offset:(offset+attr_length)]
            )
            offset += attr_length

        # Parse payload length
        if len(payload) < offset + 4:
            return None

        payload_length = struct.unpack('>I', payload[offset:offset + 4])[0]
        offset += 4

        # Extract payload data
        if len(payload) < offset + payload_length:
            return None

        packet_payload = payload[offset:offset + payload_length]

        packet = {
            'direction': header["direction"],
            'type': packet_type,
            'type_name': type_name,
            'attributes': attributes,
            'payload': packet_payload
        }

        # Parse specific content based on packet type
        if packet_type in [30, 31]:  # Video/Audio
            packet.update(ProtocolParser.parse_media_packet(packet_payload))
        elif packet_type == 32:  # Image
            packet.update(ProtocolParser.parse_image_packet(packet_payload))
        elif packet_type == 34:  # Text
            packet.update(ProtocolParser.parse_text_packet(packet_payload))

        return packet
