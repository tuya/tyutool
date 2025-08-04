#!/usr/bin/env python3
# coding=utf-8

import os
import wave

from tyutool.util.util import get_logger

logger = get_logger()


def pcm2wav(pcm_file, wav_file,
            sample_rate=16000, bits_per_sample=16, channels=1):
    if not os.path.exists(pcm_file):
        logger.error(f"Not found: {pcm_file}.")
        return

    sample_width = bits_per_sample // 8
    if sample_width not in (1, 2):
        logger.error("bits_per_sample only [8 or 16].")
        return

    try:
        with open(pcm_file, 'rb') as pcm_f:
            pcm_data = pcm_f.read()

        with wave.open(wav_file, 'wb') as wav_f:
            wav_f.setnchannels(channels)          # 声道数
            wav_f.setsampwidth(sample_width)      # 样本宽度（字节）
            wav_f.setframerate(sample_rate)       # 采样率
            wav_f.writeframes(pcm_data)           # 写入PCM数据

        logger.info(f"pcm2wav：sample_rate={sample_rate}Hz, \
bits_per_sample={bits_per_sample}bit, channels={channels}")

    except Exception as e:
        logger.error(f"pcm2wav：{str(e)}")
