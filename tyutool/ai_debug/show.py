#!/usr/bin/env python3
# -*- coding: utf-8 -*-

from datetime import datetime

from .protocol import TYPE_MAPPING

logger = None


class DataDisplay:
    def __init__(self, logger):
        self.logger = logger

    def display_packet(self, packet):
        # Display packet information
        timestamp = datetime.now().strftime('%Y-%m-%d %H:%M:%S.%f')[:-3]
        msg = f'''
{"="*60}
📅 Time: {timestamp}
📦 Type: {packet['type_name']} ({packet['type']})
'''

        # Display attributes
        if packet.get('attributes'):
            msg += "🏷️  Attributes:"
            for key, value in packet['attributes'].items():
                if isinstance(value, bytes):
                    value = f"<{len(value)} bytes>"
                msg += f"\n   - {key}: {value}"

        # Display specific content based on type
        packet_type = packet['type']
        if packet_type == 31:  # Audio
            self._display_audio(packet, msg)
        elif packet_type == 34:  # Text
            self._display_text(packet, msg)
        elif packet_type == 30:  # Video
            self._display_video(packet, msg)
        elif packet_type == 32:  # Image
            self._display_image(packet, msg)
        pass

    def _display_audio(self, packet, msg):
        data_id = packet.get('data_id', 0)
        stream_flag = packet.get('stream_flag', 0)
        direction = "Uplink" if data_id % 2 == 1 else "Downlink"

        stream_status = {
            0: "Single packet",
            1: "Stream start",
            2: "Stream continue",
            3: "Stream end",
        }
        status = stream_status.get(stream_flag, "Unknown")
        media_payload = packet.get('media_payload', b'')

        audio_msg = f'''
🎵 Audio data:
   - DataID: {data_id} ({direction})
   - Status: {status}
   - Timestamp: {packet.get('timestamp', 0)}
   - PTS: {packet.get('pts', 0)}
   - Size: {len(media_payload)} bytes
'''

        msg += audio_msg
        self.logger.info(msg)
        pass

    def _display_text(self, packet, msg):
        data_id = packet.get('data_id', 0)
        stream_flag = packet.get('stream_flag', 0)
        text_content = packet.get('text_content', '')

        stream_status = {
            0: "Single packet",
            1: "Stream start",
            2: "Stream continue",
            3: "Stream end",
        }
        status = stream_status.get(stream_flag, "Unknown")

        text_msg = f'''
📝 Text data:
   - DataID: {data_id}
   - Status: {status}
   - Content: {text_content}
'''
        msg += text_msg
        self.logger.info(msg)
        pass

    def _display_video(self, packet, msg):
        data_id = packet.get('data_id', 0)
        stream_flag = packet.get('stream_flag', 0)

        stream_status = {
            0: "Single packet",
            1: "Stream start",
            2: "Stream continue",
            3: "Stream end",
        }
        status = stream_status.get(stream_flag, "Unknown")
        media_payload = packet.get('media_payload', b'')

        video_msg = f'''
🎥 Video data:
   - DataID: {data_id}
   - Status: {status}
   - Timestamp: {packet.get('timestamp', 0)}
   - PTS: {packet.get('pts', 0)}
   - Size: {len(media_payload)} bytes
'''

        msg += video_msg
        self.logger.info(msg)
        pass

    def _display_image(self, packet, msg):
        data_id = packet.get('data_id', 0)
        timestamp = packet.get('timestamp', 0)
        format = "None"
        image_payload = packet.get('image_payload', b'')
        if image_payload.startswith(b'\xff\xd8\xff'):
            format = "JPEG"
        elif image_payload.startswith(b'\x89PNG'):
            format = "PNG"

        image_msg = f'''
🖼️  Image data:
   - DataID: {data_id}
   - Timestamp: {timestamp}
   - Format: {format}
   - Size: {len(image_payload)} bytes
'''

        msg += image_msg
        self.logger.info(msg)
        pass

    def get_type_name(self, type_char):
        return TYPE_MAPPING.get(type_char, type_char)
