#!/usr/bin/env python3
# -*- coding: utf-8 -*-

from datetime import datetime


class DataDisplay:
    def __init__(self, logger, display_hook=None):
        self.logger = logger
        self.hook = display_hook
        pass

    def _display_audio(self, packet, msg):
        data_id = packet.get('data_id', 0)
        stream_status = packet.get('stream_status', "Unknown")
        direction = "Uplink" if data_id % 2 == 1 else "Downlink"

        audio_msg = f'''
🎵 Audio data:
   - DataID: {data_id} ({direction})
   - Status: {stream_status}
   - Timestamp: {packet.get('timestamp', 0)}
   - PTS: {packet.get('pts', 0)}
   - Size: {packet.get('size', 0)} bytes
'''

        msg += audio_msg
        if self.hook is not None:
            self.hook.hook(packet, msg)
        else:
            self.logger.info(msg)

        pass

    def _display_text(self, packet, msg):
        data_id = packet.get('data_id', 0)
        stream_status = packet.get('stream_status', "Unknown")
        text_content = packet.get('text_content', '')

        text_msg = f'''
📝 Text data:
   - DataID: {data_id}
   - Status: {stream_status}
   - Content: {text_content}
'''
        msg += text_msg
        if self.hook is not None:
            self.hook.hook(packet, msg)
        else:
            self.logger.info(msg)

        pass

    def _display_video(self, packet, msg):
        data_id = packet.get('data_id', 0)
        stream_status = packet.get('stream_status', "Unknown")

        video_msg = f'''
🎥 Video data:
   - DataID: {data_id}
   - Status: {stream_status}
   - Timestamp: {packet.get('timestamp', 0)}
   - PTS: {packet.get('pts', 0)}
   - Size: {packet.get('size', 0)} bytes
'''

        msg += video_msg
        self.logger.info(msg)
        pass

    def _display_image(self, packet, msg):
        data_id = packet.get('data_id', 0)
        stream_status = packet.get('stream_status', "Unknown")
        timestamp = packet.get('timestamp', 0)
        size = packet.get('size', 0)
        format = "None"
        image_payload = packet.get('image_payload', b'')
        if image_payload.startswith(b'\xff\xd8\xff'):
            format = "JPEG"
        elif image_payload.startswith(b'\x89PNG'):
            format = "PNG"

        image_msg = f'''
🖼️  Image data:
   - DataID: {data_id}
   - Status: {stream_status}
   - Timestamp: {timestamp}
   - Format: {format}
   - Size: {size} bytes
'''

        msg += image_msg
        self.logger.info(msg)
        pass

    def display_packet(self, packet):
        # Display packet information
        timestamp = datetime.now().strftime('%Y-%m-%d %H:%M:%S.%f')[:-3]
        msg = f'''
{"="*30}
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
