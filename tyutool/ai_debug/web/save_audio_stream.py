#!/usr/bin/env python3
# coding=utf-8

import os
import base64
import struct
import traceback
from datetime import datetime

# 导入pyogg库用于Opus解码
from .libs.PyOgg.pyogg import OpusDecoder


class SaveAudioStream:
    def __init__(self, save_dir, logger):
        self.save_dir = os.path.join(save_dir, "audio_stream")
        self.logger = logger

        # 音频流管理
        self.audio_streams = {}  # 存储音频流数据，key为内部生成的stream_id
        self.completed_audio_streams = {}  # 存储完成的音频流
        self.next_stream_id = 1  # 内部流ID计数器

        # Opus解码器 (用于上行Opus数据转换成PCM)
        self.decoder = None

        # 创建保存目录
        os.makedirs(self.save_dir, exist_ok=True)

    def save(self, packet):
        """保存音频数据包"""
        if packet.get('type') == 31:  # Audio packet
            self._handle_audio_stream(packet)

    def _handle_audio_stream(self, packet):
        """处理音频流数据包"""
        try:
            data_id = packet.get('data_id', 0)
            stream_flag = packet.get('stream_flag', 0)

            # 根据协议区分上下行流：奇数为上行，偶数为下行
            direction = "upstream" if data_id % 2 == 1 else "downstream"

            self.logger.debug(f"audio stream packet - \
data_id: {data_id}, stream_flag: {stream_flag}, direction: {direction}")

            attributes = packet.get('attributes', {})  # 获取属性，可能包含采样率、通道数等信息
            sample_rate = attributes.get('AudioSampleRate', 16000)
            channels = attributes.get('AudioChannels', 1)
            codec_type = attributes.get('AudioCodecType', 0)
            self.logger.debug(f"audio stream attributes - \
sample_rate: {sample_rate}, channels: {channels}, codec_type: {codec_type}")

            # 如果是opus音频流，则直接通过pyogg解码成PCM帧，方便处理
            if codec_type == 111:  # Opus
                self._handle_opus_decode(
                    packet, stream_flag, sample_rate, channels
                )

                # 替换音频流属性中的编解码类型为PCM
                codec_type = 101  # PCM
                attributes['AudioCodecType'] = codec_type
                packet['attributes'] = attributes

            # 流开始 (stream_flag = 1) - 创建新的音频流
            if stream_flag == 1:
                self._handle_stream_start(packet, data_id, direction)

            # 流继续 (stream_flag = 2) 或单包流 (stream_flag = 0) - 添加到最新的活跃流
            elif stream_flag in [0, 2]:
                self._handle_stream_continue(
                    packet, data_id, direction, stream_flag
                )

            # 流结束 (stream_flag = 3) - 结束最新的活跃流
            elif stream_flag == 3:
                self._handle_stream_end(
                    packet, data_id, direction
                )

        except Exception as e:
            self.logger.error(f"Audio Stream: {e}")
            self.logger.error(f"Traceback: {traceback.format_exc()}")

    def _handle_opus_decode(self, packet, stream_flag, sample_rate, channels):
        """处理Opus解码成PCM的特殊逻辑"""
        try:
            if stream_flag == 1:
                self.decoder = OpusDecoder()
                self.decoder.set_sampling_frequency(sample_rate)
                self.decoder.set_channels(channels)
            elif stream_flag in [0, 2]:
                # 如果流继续但没有解码器，则创建一个新的解码器
                if self.decoder is None:
                    self.decoder = OpusDecoder()
                    self.decoder.set_sampling_frequency(sample_rate)
                    self.decoder.set_channels(channels)

            if packet.get('media_payload') and self.decoder:
                # 处理音频数据：如果是字符串则是base64编码，如果是bytes则是原始数据
                audio_data = packet['media_payload']
                self.logger.debug(f"audio_data: {audio_data.hex()}")

                toc = audio_data[0]  # 获取TOC字节
                config = toc >> 3  # 获取配置信息（高5位）
                c = toc & 0x3  # 获取c位（低2位）
                if (c == 0x03):
                    # 一个包中有任意帧音频
                    # 获取帧数（第3位）
                    frame_count = audio_data[1] & 0x3F  # 获取帧数（低6位）
                else:
                    # 计算帧数
                    frame_count = 1  # TODO: 这里需要根据实际情况计算帧数

                # OPUS_ENCODED_BYTES_PER_FRAME = 40
                OPUS_ENCODED_BYTES_PER_FRAME = 80
                frame_size = OPUS_ENCODED_BYTES_PER_FRAME
                self.logger.debug(f"TOC bytes: {toc:#02x}, config: {config}, \
frame_count: {frame_count}, frame_size: {frame_size}")

                pcm_data = bytes()  # 用于存储解码后的PCM数据
                # 使用pyogg解码音频数据
                while len(audio_data) >= OPUS_ENCODED_BYTES_PER_FRAME:
                    frame = bytearray(
                        audio_data[:OPUS_ENCODED_BYTES_PER_FRAME]
                    )
                    audio_data = audio_data[OPUS_ENCODED_BYTES_PER_FRAME:]
                    decoded_data = self.decoder.decode(frame)
                    decoded_data = bytes(decoded_data)  # 转换为字节
                    pcm_data += decoded_data
                    self.logger.debug(f'decoded_data length: \
{len(decoded_data)}')

                if len(audio_data) > 0:
                    self.logger.warning(f"Opus to PCM remain: \
{len(audio_data)}")

                # 替换 packet['media_payload'] 为解码后的PCM数据
                self.logger.debug(f"Opus to PCM - length: \
{len(pcm_data)} bytes")
                packet['media_payload'] = pcm_data

        except Exception as e:
            self.logger.error(f"Opus decode: {e}")
            self.logger.error(f"Traceback: {traceback.format_exc()}")

    def _handle_stream_start(self, packet, data_id, direction):
        """处理音频流开始"""
        # 生成新的内部流ID
        internal_stream_id = self.next_stream_id
        self.next_stream_id += 1

        # 创建新的音频流，使用内部ID作为唯一标识
        self.audio_streams[internal_stream_id] = {
            'id': internal_stream_id,
            'data_id': data_id,  # 保留原始data_id用于调试
            'direction': direction,  # 标识流方向
            'chunks': [],
            'attributes': packet.get('attributes', {}),
            'start_time': datetime.now(),
            'total_size': 0,
            'is_active': True,
            'current_stream_id': internal_stream_id  # 标记当前活跃的流ID
        }

        # 添加第一个音频数据块（流开始包通常也包含数据）
        if packet.get('media_payload'):
            self._add_audio_chunk(internal_stream_id, packet)

        self.logger.info(f"Audio stream {internal_stream_id} start, \
data_id: {data_id}, direction: {direction}, \
attributes: {packet.get('attributes', {})}")

    def _handle_stream_continue(self, packet, data_id, direction, stream_flag):
        """处理音频流继续"""
        # 查找最新的活跃流（同data_id且同方向）
        active_stream_id = self._find_active_stream(data_id, direction)

        if active_stream_id is None:
            self.logger.warning(f"Receive continue data_id {data_id}\
(direction: {direction}, stream_flag: {stream_flag}) \
not a active stream, skip.")
            return

        # 添加音频数据块到活跃流
        if packet.get('media_payload'):
            self._add_audio_chunk(active_stream_id, packet)

    def _handle_stream_end(self, packet, data_id, direction):
        """处理音频流结束"""
        # 查找最新的活跃流（同data_id且同方向）
        active_stream_id = self._find_active_stream(data_id, direction)

        if active_stream_id is None:
            self.logger.warning(f"Receive end data_id {data_id}\
(direction: {direction}) not a active stream, skip.")
            return

        # 添加最后的音频数据块（如果有）
        if packet.get('media_payload'):
            self._add_audio_chunk(active_stream_id, packet)

        # 结束流
        stream = self.audio_streams[active_stream_id]
        stream['is_active'] = False
        stream['end_time'] = datetime.now()

        # 移动到完成的流列表
        self.completed_audio_streams[active_stream_id] = stream
        # 从活跃流列表中移除
        del self.audio_streams[active_stream_id]

        self.logger.info(f"Audio stream {active_stream_id} end, \
data_id: {data_id}, total: {len(stream['chunks'])}, \
size: {stream['total_size']} bytes")

        # 自动保存到文件
        self._save_audio_stream_to_file(stream)

    def _find_active_stream(self, data_id, direction):
        """查找指定data_id和方向的最新活跃流"""
        active_stream_id = None
        latest_start_time = None

        for stream_id, stream in self.audio_streams.items():
            if (stream.get('is_active', False)
                    and stream.get('data_id') == data_id
                    and stream.get('direction') == direction):  # 增加方向匹配
                if (latest_start_time is None
                        or stream['start_time'] > latest_start_time):
                    latest_start_time = stream['start_time']
                    active_stream_id = stream_id

        return active_stream_id

    def _add_audio_chunk(self, stream_id, packet):
        """添加音频数据块到指定流"""
        stream = self.audio_streams[stream_id]

        # 处理音频数据：如果是字符串则是base64编码，如果是bytes则是原始数据
        media_data = packet['media_payload']
        if isinstance(media_data, str):
            # 已经是base64编码的字符串，直接存储
            audio_data = media_data
            try:
                decoded_size = len(base64.b64decode(media_data))
            except Exception as e:
                self.logger.warning(f"decode base64 audio data: {e}")
                decoded_size = 0
        else:
            # 原始bytes数据，需要编码为base64
            audio_data = base64.b64encode(media_data).decode('ascii')
            decoded_size = len(media_data)

        chunk_data = {
            'data': audio_data,
            'timestamp': packet.get('timestamp'),
            'pts': packet.get('pts'),
            'sequence': len(stream['chunks'])
        }
        stream['chunks'].append(chunk_data)
        stream['total_size'] += decoded_size

        self.logger.debug(f"add audio chunk {stream_id}, \
total: {len(stream['chunks'])}, size: {stream['total_size']} bytes")

    def _save_audio_stream_to_file(self, stream):
        """将音频流保存到文件，根据编码格式生成不同类型的文件"""
        try:
            # 获取流信息
            stream_id = stream['id']
            data_id = stream.get('data_id', 'unknown')
            start_time = stream['start_time'].strftime('%Y%m%d_%H%M%S')
            attributes = stream.get('attributes', {})
            codec_type = attributes.get('AudioCodecType', 0)
            sample_rate = attributes.get('AudioSampleRate', 16000)
            channels = attributes.get('AudioChannels', 1)
            bit_depth = attributes.get('AudioBitDepth', 16)

            # 解码并合并所有音频块
            audio_data = b''
            for chunk in stream['chunks']:
                try:
                    decoded_chunk = base64.b64decode(chunk['data'])
                    audio_data += decoded_chunk
                except Exception as e:
                    self.logger.warning(f"chunk decode: {e}")

            if not audio_data:
                self.logger.warning(f"Audio stream {stream_id} no data")
                return

            # 根据编码类型生成不同格式的文件
            if codec_type == 101:  # PCM
                self._save_as_wav(audio_data, stream_id, data_id,
                                  start_time, sample_rate, channels, bit_depth)
            elif codec_type == 109:  # MP3
                self._save_as_mp3(audio_data, stream_id, data_id,
                                  start_time)
            elif codec_type in [102, 103, 104]:  # AAC variants
                self._save_as_aac(audio_data, stream_id, data_id,
                                  start_time, codec_type)
            elif codec_type == 111:  # Opus
                self._save_as_opus(audio_data, stream_id, data_id,
                                   start_time)
            else:
                # 其他格式保存为原始数据
                self._save_as_raw(audio_data, stream_id, data_id,
                                  start_time, codec_type)

        except Exception as e:
            self.logger.error(f"Save audio stream to file: {e}")
            self.logger.error(f"Traceback: {traceback.format_exc()}")

    def _save_as_wav(self, audio_data, stream_id, data_id,
                     timestamp, sample_rate, channels, bit_depth):
        """保存为WAV格式（添加WAV头）"""
        try:
            # 计算相关参数
            byte_rate = sample_rate * channels * bit_depth // 8
            block_align = channels * bit_depth // 8
            data_size = len(audio_data)
            file_size = 36 + data_size

            # 构造WAV头部
            wav_header = b''
            # RIFF头
            wav_header += b'RIFF'
            wav_header += struct.pack('<I', file_size)  # 文件大小-8
            wav_header += b'WAVE'

            # fmt chunk
            wav_header += b'fmt '
            wav_header += struct.pack('<I', 16)  # fmt chunk size
            wav_header += struct.pack('<H', 1)   # audio format (PCM)
            wav_header += struct.pack('<H', channels)
            wav_header += struct.pack('<I', sample_rate)
            wav_header += struct.pack('<I', byte_rate)
            wav_header += struct.pack('<H', block_align)
            wav_header += struct.pack('<H', bit_depth)

            # data chunk
            wav_header += b'data'
            wav_header += struct.pack('<I', data_size)

            # 保存文件
            filename = f"{stream_id}_data{data_id}_{timestamp}.wav"
            filepath = os.path.join(self.save_dir, filename)

            with open(filepath, 'wb') as f:
                f.write(wav_header)
                f.write(audio_data)

            self.logger.info(f"Audio stream {stream_id} save to WAV file: \
{filepath} ({len(audio_data)} bytes PCM + WAV header)")

        except Exception as e:
            self.logger.error(f"Save WAV file: {e}")

    def _save_as_mp3(self, audio_data, stream_id, data_id, timestamp):
        """保存为MP3格式（直接保存，因为数据已经是MP3编码）"""
        try:
            filename = f"{stream_id}_data{data_id}_{timestamp}.mp3"
            filepath = os.path.join(self.save_dir, filename)

            with open(filepath, 'wb') as f:
                f.write(audio_data)

            self.logger.info(f"Audio stream {stream_id} save to MP3 file: \
{filepath} ({len(audio_data)} bytes)")

        except Exception as e:
            self.logger.error(f"Save MP3 file: {e}")

    def _save_as_aac(self, audio_data, stream_id, data_id,
                     timestamp, codec_type):
        """保存为AAC格式"""
        try:
            # 根据AAC类型确定扩展名
            if codec_type == 102:  # AAC Raw
                ext = "aac"
            elif codec_type == 103:  # AAC ADTS
                ext = "aac"
            elif codec_type == 104:  # AAC LATM
                ext = "latm"
            else:
                ext = "aac"

            filename = f"{stream_id}_data{data_id}_{timestamp}.{ext}"
            filepath = os.path.join(self.save_dir, filename)

            with open(filepath, 'wb') as f:
                f.write(audio_data)

            self.logger.info(f"Audio stream {stream_id} save to AAC file: \
{filepath} ({len(audio_data)} bytes)")

        except Exception as e:
            self.logger.error(f"Save AAC file: {e}")

    def _save_as_opus(self, audio_data, stream_id, data_id, timestamp):
        """保存为Opus格式"""
        try:
            filename = f"{stream_id}_data{data_id}_{timestamp}.opus"
            filepath = os.path.join(self.save_dir, filename)

            with open(filepath, 'wb') as f:
                f.write(audio_data)

            self.logger.info(f"Audio stream {stream_id} save to Opus file: \
{filepath} ({len(audio_data)} bytes)")

        except Exception as e:
            self.logger.error(f"Save Opus file: {e}")

    def _save_as_raw(self, audio_data, stream_id, data_id,
                     timestamp, codec_type):
        """保存为原始格式"""
        try:
            # 获取编码类型名称
            codec_names = {
                100: 'adpcm', 105: 'g711u', 106: 'g711a', 107: 'g726',
                108: 'speex', 110: 'g722'
            }

            codec_name = codec_names.get(codec_type, f'codec{codec_type}')
            filename = f"{stream_id}_data{data_id}_{timestamp}.{codec_name}"
            filepath = os.path.join(self.save_dir, filename)

            with open(filepath, 'wb') as f:
                f.write(audio_data)

            self.logger.info(f"Audio stream {stream_id} save to RAW file: \
{filepath} ({len(audio_data)} bytes, codec: {codec_name})")

        except Exception as e:
            self.logger.error(f"Save RAW file: {e}")
