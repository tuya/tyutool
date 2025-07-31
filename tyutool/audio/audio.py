import wave
import os


def pcm_to_wav(pcm_file, wav_file,
               sample_rate=16000, bits_per_sample=16, channels=1):
    # 检查PCM文件是否存在
    if not os.path.exists(pcm_file):
        print(f"错误：PCM文件 {pcm_file} 不存在")
        return

    # 计算样本宽度（字节数）
    sample_width = bits_per_sample // 8
    if sample_width not in (1, 2):
        print("错误：位深仅支持8或16")
        return

    try:
        # 读取PCM原始数据
        with open(pcm_file, 'rb') as pcm_f:
            pcm_data = pcm_f.read()

        # 创建并写入WAV文件
        with wave.open(wav_file, 'wb') as wav_f:
            # 设置WAV文件参数
            wav_f.setnchannels(channels)          # 声道数
            wav_f.setsampwidth(sample_width)      # 样本宽度（字节）
            wav_f.setframerate(sample_rate)       # 采样率
            wav_f.writeframes(pcm_data)           # 写入PCM数据

        print(f"转换成功：{wav_file}")
        print(f"参数：采样率={sample_rate}Hz, 位深={bits_per_sample}bit, 声道数={channels}")

    except Exception as e:
        print(f"转换失败：{str(e)}")
