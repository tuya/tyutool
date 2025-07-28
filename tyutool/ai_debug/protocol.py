#!/usr/bin/env python3
# coding=utf-8

import struct

from tyutool.util.util import get_logger

logger = get_logger()


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

    @staticmethod
    def _find_frame_sync(data):
        """Frame sync - Find magic field in data stream"""
        expected_magic = 0x54594149  # "TYAI"

        for i in range(1, len(data) - 3):
            if len(data) >= i + 4:
                magic = struct.unpack('>I', data[i:i+4])[0]
                if magic == expected_magic:
                    logger.debug(f"Found magic field at offset {i}.")
                    return ProtocolParser.parse_transport_header(data[i:])

        logger.debug("Magic field not found.")
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
            logger.debug("Magic field mismatch:")
            logger.debug(f"expected 0x{expected_magic:08x}, \
actual 0x{magic:08x}")
            return ProtocolParser._find_frame_sync(data)

        # 0:device->cloud，1:cloud->device，2:device->debuger
        direction = (data[4] & 0xC0) >> 6  # 0x11000000

        version = data[5]
        if version != 1:
            logger.debug(f"Unsupported version: {version}")
            return ProtocolParser._find_frame_sync(data[4:])

        sequence = struct.unpack('>H', data[6:8])[0]

        byte7 = data[8]
        frag_flag = (byte7 >> 6) & 0x03  # 0x00000011
        security_level = (byte7 >> 1) & 0x1F  # 0x00011111 must 0
        iv_flag = byte7 & 0x01  # 0x00000001

        if security_level != 0 or iv_flag != 0:
            logger.debug(f"Unsupported security level: {security_level}")
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
            logger.error(f"Invalid payload length: {payload_length}")
            return None, data

        header['payload'] = data[offset:(offset+payload_length)]
        offset += payload_length

        header['signature'] = data[offset:(offset+signature_length)]

        return header, data[total_needed:]
