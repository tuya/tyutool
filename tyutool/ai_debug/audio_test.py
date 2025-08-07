import os
import json
import numpy as np
from scipy.signal import coherence
from scipy.fft import rfft, rfftfreq
import matplotlib.pyplot as plt


class AudioTestTools():
    def __init__(self, save_path, logger):
        self.save_path = save_path
        self.logger = logger
        pass

    def load_signals(self, mic1_path, mic2_path, ref_path,
                     sr=16000, dtype='int16'):
        def read_pcm(path):
            data = np.fromfile(path, dtype=dtype)
            return data.astype(np.float32) / np.iinfo(dtype).max
        mic1 = read_pcm(mic1_path)
        mic2 = read_pcm(mic2_path)
        ref = read_pcm(ref_path)

        def split_pcm(data):
            frame = 640
            blocks = data.size//frame
            idx = 0
            for i in range(blocks):
                chunk = data[i*frame:i*frame+frame]
                energy = np.mean(chunk**2)
                if energy >= 0.0001:
                    idx = i*frame
                    break
            return idx

        loc = split_pcm(ref)
        return (mic1[loc:loc+16000*2], mic2[loc:loc+16000*2],
                ref[loc:loc+16000*2], sr)

    def dc_offset(self, signal):
        if signal is None:
            return 0
        else:
            dc = np.mean(signal)
            if dc > 0.01:
                ans = False
            else:
                ans = True
            return ans, dc

    def mic_coherence(self, mic1, mic2, ref, sr):
        plt.figure(figsize=(8, 4))
        # 对齐信号（以ref为基准，mic1/mic2与ref做互相关，找到最大相关位置进行对齐）
        mic1_aligned, ref1 = self.align_signal(mic1, ref)
        f, Cxy_1 = coherence(
            mic1_aligned, ref1, fs=sr, window='hann',
            nperseg=1024, noverlap=512
        )
        f, Cxy_3 = coherence(
            ref, ref, fs=sr, window='hann', nperseg=1024, noverlap=512
        )
        plt.semilogx(f, Cxy_1, label='Mic1-Ref')
        plt.semilogx(f, Cxy_3, label='Ref-Ref', linestyle='--', color='gray')
        if mic2 is not None:
            mic2_aligned, ref2 = self.align_signal(mic2, ref)
            f, Cxy_2 = coherence(
                mic2_aligned, ref2, fs=sr, window='hann',
                nperseg=1024, noverlap=512
            )
            plt.semilogx(f, Cxy_2, label='Mic2-Ref')

        plt.xlabel('Frequency (Hz)')
        plt.ylabel('Coherence')
        plt.title('Microphone Coherence')
        plt.legend()
        plt.grid(True, which='both', ls='--')
        # plt.show()
        save_path = os.path.join(self.save_path, 'mic_coherence.png')
        plt.savefig(save_path)
        if mic2 is not None:
            Cxy = (Cxy_1 + Cxy_2) / 2
        else:
            Cxy = Cxy_1

        if np.mean(Cxy[(f >= 100) & (f < 4000)]) > 0.7:
            ans = True
        else:
            ans = False
        return ans, np.mean(Cxy[f < 4000])

    # 对齐信号
    def align_signal(self, sig, ref):
        # Downsample for faster cross-correlation
        ds_factor = 8
        sig_ds = sig[::ds_factor]
        ref_ds = ref[::ds_factor]
        corr = np.correlate(sig_ds, ref_ds, mode='full')
        delay_ds = np.argmax(corr) - len(ref_ds) + 1
        delay = delay_ds * ds_factor
        if delay > 0:
            aligned = sig[delay:]
            ref_aligned = ref[:len(aligned)]
        elif delay < 0:
            aligned = sig[:len(sig)+delay]
            ref_aligned = ref[-delay:len(ref)+delay]
        else:
            aligned = sig
            ref_aligned = ref
        min_len = min(len(aligned), len(ref_aligned))
        return aligned[:min_len], ref_aligned[:min_len]

    # 底噪
    def noise_floor(self, signal):
        # 计算RMS噪声的dB值（相对于满刻度，FS）
        rms = np.sqrt(np.mean(np.abs(signal)**2))
        db = 20 * np.log10(rms + 1e-12)  # 防止log(0)
        self.logger.debug(f"Noise floor: {db:.2f} dBFS")
        if db > -50:
            ans = False
        else:
            ans = True
        return ans, db

    # 削波检测
    def clip_detect(self, signal):
        # Return the number of clipped samples (>= 99% of full scale)
        threshold = 0.99
        clipped_samples = np.sum(np.abs(signal) >= threshold)
        if clipped_samples > 0:
            ans = False
        else:
            ans = True
        return ans, clipped_samples

    # 总谐波失真
    def thd(self, signal, sr, freq):
        # Calculate THD at given fundamental frequency
        N = len(signal)
        yf = np.abs(rfft(signal))
        xf = rfftfreq(N, 1/sr)
        fund_idx = np.argmin(np.abs(xf - freq))
        fund_power = yf[fund_idx]
        harmonics = [2, 3, 4, 5]
        harm_power = 0
        for h in harmonics:
            idx = np.argmin(np.abs(xf - h*freq))
            harm_power += yf[idx]**2
        thd_value = np.sqrt(harm_power) / fund_power
        if thd_value > 0.05:
            ans = False
        else:
            ans = True
        return ans, thd_value

    # 延时稳定性
    def plot_delay_stability_over_time(self, mic1, mic2, ref, sr,
                                       window_ms=300, hop_ms=100,
                                       save_file='delay_stability.png',
                                       need_show=False):
        """
        每window_ms毫秒计算一次延时,窗移hop_ms毫秒,画出延时随时间变化的曲线,并保存图片。
        """
        window_size = int((window_ms / 1000) * sr)
        hop_size = int((hop_ms / 1000) * sr)
        num_windows = (len(mic1) - window_size) // hop_size + 1
        times = []
        delays1 = []
        delays2 = []
        for i in range(num_windows):
            start = i * hop_size
            end = start + window_size
            mic1_win = mic1[start:end]
            mic2_win = mic2[start:end]
            ref_win = ref[start:end]
            corr1 = np.correlate(mic1_win, ref_win, mode='full')
            delay_samples1 = np.argmax(corr1) - len(ref_win) + 1
            corr2 = np.correlate(mic2_win, ref_win, mode='full')
            delay_samples2 = np.argmax(corr2) - len(ref_win) + 1
            delays1.append(delay_samples1)
            delays2.append(delay_samples2)
            times.append(start / sr)
        diff1 = np.diff(delays1, 1)
        diff2 = np.diff(delays2, 1)
        if np.any(np.abs(diff1) > 2) or np.any(np.abs(diff2) > 2):
            ans = False
        else:
            ans = True
        plt.figure(figsize=(8, 4))
        plt.plot(times, delays1, label='Mic1-Ref')
        plt.plot(times, delays2, label='Mic2-Ref')
        plt.xlabel('Time (s)')
        plt.ylabel('Delay (samples)')
        plt.title('Delay Stability Over Time (win_len:300ms, overlap:100ms)')
        plt.legend()
        plt.grid(True)
        save_path = os.path.join(self.save_path, save_file)
        plt.savefig(save_path)
        self.logger.info("\nfigure saved at delay_stability.png")
        if need_show:
            plt.show()
        return ans

    def test_all(self, k1_mic1, k1_mic2, k1_ref,
                 white_mic1, white_mic2, white_ref,
                 silence_mic1, silence_mic2, silence_ref):
        report = {}

        # 1K-0dB
        mic1, mic2, ref, sr = self.load_signals(k1_mic1, k1_mic2, k1_ref)
        freq = 1000
        mic1_dc_ans, mic1_dc_dc = self.dc_offset(mic1)
        mic2_dc_ans, mic2_dc_dc = self.dc_offset(mic2)
        ref_dc_ans, ref_dc_dc = self.dc_offset(ref)
        mic1_thd_ans, mic1_thd_thd = self.thd(mic1, sr, freq)
        mic2_thd_ans, mic2_thd_thd = self.thd(mic2, sr, freq)
        ref_thd_ans, ref_thd_thd = self.thd(ref, sr, freq)
        report["1K-0dB"] = {
            "DC-Offset": {
                "mic1_pass": mic1_dc_ans,
                "mic1_dc_value": f"{mic1_dc_dc:.6f}",
                "mic2_pass": mic2_dc_ans,
                "mic2_dc_value": f"{mic2_dc_dc:.6f}",
                "ref_pass": ref_dc_ans,
                "ref_dc_value": f"{ref_dc_dc:.6f}",
            },
            "THD": {
                "mic1_pass": mic1_thd_ans,
                "mic1_thd": f"{mic1_thd_thd:.4f}",
                "mic2_pass": mic2_thd_ans,
                "mic2_thd": f"{mic2_thd_thd:.4f}",
                "ref_pass": ref_thd_ans,
                "ref_thd": f"{ref_thd_thd:.4f}",
            },
        }

        # silence
        mic1, mic2, ref, sr = self.load_signals(silence_mic1, silence_mic2,
                                                silence_ref)
        mic1_dc_ans, mic1_dc_dc = self.dc_offset(mic1)
        mic2_dc_ans, mic2_dc_dc = self.dc_offset(mic2)
        ref_dc_ans, ref_dc_dc = self.dc_offset(ref)
        mic1_db_ans, mic1_db_db = self.noise_floor(mic1)
        mic2_db_ans, mic2_db_db = self.noise_floor(mic2)
        report["Silence"] = {
            "DC-Offset": {
                "mic1_pass": mic1_dc_ans,
                "mic1_dc_value": f"{mic1_dc_dc:.6f}",
                "mic2_pass": mic2_dc_ans,
                "mic2_dc_value": f"{mic2_dc_dc:.6f}",
                "ref_pass": ref_dc_ans,
                "ref_dc_value": f"{ref_dc_dc:.6f}",
            },
            "NoiseFloor": {
                "mic1_pass": mic1_db_ans,
                "mic1_db": f"{mic1_db_db:.6f}",
                "mic2_pass": mic2_db_ans,
                "mic2_db": f"{mic2_db_db:.6f}",
            },
        }

        # white
        mic1, mic2, ref, sr = self.load_signals(white_mic1, white_mic2,
                                                white_ref)
        mic1_dc_ans, mic1_dc_dc = self.dc_offset(mic1)
        mic2_dc_ans, mic2_dc_dc = self.dc_offset(mic2)
        ref_dc_ans, ref_dc_dc = self.dc_offset(ref)
        coherence_ans, coherence_mean = self.mic_coherence(mic1, mic2, ref, sr)
        mic1_clipped, mic1_samples = self.clip_detect(mic1)
        mic2_clipped, mic2_samples = self.clip_detect(mic2)
        delay_stability = self.plot_delay_stability_over_time(mic1, mic2,
                                                              ref, sr)
        report["White"] = {
            "DC-Offset": {
                "mic1_pass": mic1_dc_ans,
                "mic1_dc_value": f"{mic1_dc_dc:.6f}",
                "mic2_pass": mic2_dc_ans,
                "mic2_dc_value": f"{mic2_dc_dc:.6f}",
                "ref_pass": ref_dc_ans,
                "ref_dc_value": f"{ref_dc_dc:.6f}",
            },
            "Coherence": {
                "coherence_pass": coherence_ans,
                "coherence_mean": f"{coherence_mean:.3f}",
            },
            "SignalIntegrity": {
                "mic1_pass": mic1_clipped,
                "mic1_samples": int(mic1_samples),
                "mic2_pass": mic2_clipped,
                "mic2_samples": int(mic2_samples),
            },
            "DelayStability": {
                "delay_stability_pass": delay_stability,
            },
        }

        report_format = json.dumps(report, indent=4, ensure_ascii=False)
        self.logger.info(f"report: \n{report_format}")
        save_file = os.path.join(self.save_path, "report.json")
        with open(save_file, 'w') as f:
            f.write(report_format)

        pass
