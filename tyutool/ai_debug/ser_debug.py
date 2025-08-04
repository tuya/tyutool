#!/usr/bin/env python3
# coding=utf-8

import os
import sys
import time
import serial
import threading
from datetime import datetime

from .audio_test import AudioTestTools
from tyutool.cli import choose_port


class SerAIDebugMonitor(object):
    def __init__(self, port, baud, save,
                 logger):
        self.logger = logger
        self.port = port or choose_port()
        self.baud = baud
        now = datetime.now().strftime("%Y%m%d-%H%M%S")
        save_path = os.path.join(save, now)
        os.makedirs(save_path, exist_ok=True)
        self.save_path = os.path.abspath(save_path)
        self.logger.debug(f"save_path: {save_path}.")

        self.ser = None
        self.running = False
        self.dump_files = {
            '0': {'name': 'dump_mic.pcm',
                  'file': None, 'active': False, 'length': 0},
            '1': {'name': 'dump_ref.pcm',
                  'file': None, 'active': False, 'length': 0},
            '2': {'name': 'dump_aec.pcm',
                  'file': None, 'active': False, 'length': 0}
        }
        self.current_dump_channel = None
        self.last_data_time = 0
        pass

    def open_port(self):
        try:
            self.ser = serial.Serial(
                port=self.port,
                baudrate=self.baud,
                timeout=0.1
            )
            time.sleep(0.5)  # 等待串口初始化
        except Exception as e:
            self.logger.error(f"Open serial: {e}")
            return False
        return True

    def start_reading(self):
        self.running = True
        self.read_thread = threading.Thread(target=self.read_from_serial)
        self.read_thread.daemon = True
        self.read_thread.start()
        pass

    def stop_reading(self):
        self.running = False
        if hasattr(self, 'read_thread') and self.read_thread.is_alive():
            self.read_thread.join()
        pass

    def read_from_serial(self):
        while self.running:
            if self.ser and self.ser.is_open:
                try:
                    if self.ser.in_waiting > 0:
                        data = self.ser.read(self.ser.in_waiting)
                        self.last_data_time = time.time()  # 更新最后接收时间
                        self.process_received_data(data)
                except Exception as e:
                    self.logger.error(f"read serial: {e}")

            # 检查dump超时
            if self.current_dump_channel:
                if time.time() - self.last_data_time > 0.1:  # 100ms超时
                    self.stop_dump()

            time.sleep(0.01)
        pass

    def process_received_data(self, data):
        # 非dump模式下显示接收数据
        if not self.current_dump_channel:
            try:
                print(data.decode('utf-8', errors='replace'), end='')
            except Exception as e:
                self.logger.debug(f"decode error: {e}")
                pass  # 忽略非文本数据
            return

        # 更新dump文件长度
        dump_active = self.dump_files[self.current_dump_channel]['active']
        if dump_active:
            self.dump_files[self.current_dump_channel]['length'] += len(data)
            self.update_length_display()

        # 写入当前dump文件
        dump_file = self.dump_files[self.current_dump_channel]['file']
        if dump_file:
            dump_file.write(data)
        pass

    def update_length_display(self):
        """更新dump长度显示（同一行刷新）"""
        if self.current_dump_channel:
            channel = self.current_dump_channel
            length = self.dump_files[channel]['length']
            times = length/32000
            sys.stdout.write(f"\rReceived: {length} bytes ({times:.3f}s)")
            sys.stdout.flush()
        pass

    def send_command(self, cmd):
        if not self.ser or not self.ser.is_open:
            self.logger.warning("The serial port is not connected.")
            return False

        try:
            # 添加"ao "前缀和回车换行
            full_cmd = f"ao {cmd}\r\n"
            self.ser.write(full_cmd.encode('utf-8'))
            self.logger.info(f"Send: ao {cmd}")
        except Exception as e:
            self.logger.error(f"Send failed: {e}")
            return False
        return True

    def start_dump(self, channel, prefix):
        if channel not in self.dump_files:
            self.logger.warning(f"Invalid channel: {channel}")
            return

        # 如果已有dump在运行，先停止
        self.stop_dump()

        # 发送dump命令
        self.send_command(f"dump {channel}")

        try:
            # 打开文件（二进制写入），覆盖已存在的文件
            prefix = prefix or datetime.now().strftime("%H%M%S")
            file_name = f"{prefix}-{self.dump_files[channel]['name']}"
            file_path = os.path.join(self.save_path, file_name)
            self.dump_files[channel]['file'] = open(file_path, 'wb')
            self.dump_files[channel]['active'] = True
            self.dump_files[channel]['length'] = 0
            self.current_dump_channel = channel
            self.last_data_time = time.time()  # 记录开始时间

            self.logger.info(f"dump {channel} -> {file_path}")
            self.logger.info("Waiting... (timeout 100ms)")
        except Exception as e:
            self.logger.error(f"dump {channel}: {e}")
        pass

    def stop_dump(self):
        if not self.current_dump_channel:
            return

        channel = self.current_dump_channel
        if channel and self.dump_files[channel]['active']:
            if self.dump_files[channel]['file']:
                self.dump_files[channel]['file'].close()
            self.dump_files[channel]['file'] = None
            self.dump_files[channel]['active'] = False

            length = self.dump_files[channel]['length']
            self.logger.info(f"\nStop dump {channel} ({length} bytes)")
            self.logger.info(f"Saved to: {self.dump_files[channel]['name']}")

            # 重置当前通道
            self.current_dump_channel = None
        pass

    def close_port(self):
        self.stop_dump()

        if self.ser and self.ser.is_open:
            self.ser.close()
            self.logger.info("The serial port has been closed.")

    def show_help(self):
        print("\nSupport commands:")
        print("start       - Start recording")
        print("stop        - Stop recording")
        print("reset       - Reset recording")
        print("dump 0      - Dump microphone channel")
        print("dump 1      - Dump reference channel")
        print("dump 2      - Dump AEC channel")
        print("bg 0        - white noise")
        print("bg 1        - 1K-0dB (bg 1 1000)")
        print("bg 2        - sweep frequency constantly")
        print("bg 3        - sweep discrete frequency")
        print("bg 4        - min single frequency")
        print("volume <70>   - Set volume to 70%")
        print("micgain <70>   - Default micgain=70")
        print("alg set <para> <value> \
- Set audio algorithm parameters (e.g.: alg set aec_ec_depth 1)")
        print("alg set vad_SPthr <0-13> <value> \
- Set audio algorithm parameters (e.g.: alg set vad_SPthr 0 1000)")
        print("alg get <para> \
- Get audio algorithm parameters (e.g.: alg get aec_ec_depth)")
        print("alg dump    - Dump audio algorithm parameters")
        print("quit        - Exit the program")
        pass

    def process_input_cmd(self, cmd):
        # Quit
        if cmd == 'quit':
            return False

        # Help
        if cmd.startswith('help'):
            self.show_help()
            return True

        # Dump
        if cmd in ['dump 0', 'dump 1', 'dump 2']:
            channel = cmd.split()[1]
            self.start_dump(channel)
            return True

        # Check [alg set] params
        elif cmd.startswith('alg set '):
            parts = cmd.split()
            if len(parts) == 4:
                self.send_command(f"alg set {parts[2]} {parts[3]}")
            elif len(parts) == 5 and "vad_SPthr" == parts[2]:
                # SPthr 参数需要两个值
                idx = parts[3]  # 0~13
                value = parts[4]  # 0~65535
                # check if idx is a number
                if idx.isdigit() and 0 <= int(idx) <= 13:
                    # 拼接命令 spthr_idx[8] || rev[8] || value[16]
                    alg_parm = (int(idx) << 24) | (int(value) & 0xFFFF)
                    alg_cmd = f"alg set vad_SPthr {alg_parm}"
                    self.send_command(alg_cmd)
                    self.logger.info(f"->: {alg_cmd}")
                else:
                    self.logger.error("<idx> should be between 0 and 13")
            else:
                self.logger.error("Command error")
            return True

        # Other
        self.send_command(cmd)
        return True

    def auto_test(self):
        '''
        '''
        cmd_sleep = 0.1
        play_sleep = 5
        dump_sleep = 10

        # white
        self.send_command("start")
        time.sleep(cmd_sleep)
        self.send_command("bg 0")
        time.sleep(play_sleep)
        self.send_command("stop")
        time.sleep(cmd_sleep)
        self.start_dump("0", "white")
        time.sleep(dump_sleep)
        self.start_dump("1", "white")
        time.sleep(dump_sleep)

        file_name = f"white-{self.dump_files['0']['name']}"
        white_mic1 = os.path.join(self.save_path, file_name)
        white_mic2 = white_mic1
        file_name = f"white-{self.dump_files['1']['name']}"
        white_ref = os.path.join(self.save_path, file_name)

        # 1K-0dB
        self.send_command("start")
        time.sleep(cmd_sleep)
        self.send_command("bg 1")
        time.sleep(play_sleep)
        self.send_command("stop")
        time.sleep(cmd_sleep)
        self.start_dump("0", "1k")
        time.sleep(dump_sleep)
        self.start_dump("1", "1k")
        time.sleep(dump_sleep)

        file_name = f"1k-{self.dump_files['0']['name']}"
        k1_mic1 = os.path.join(self.save_path, file_name)
        k1_mic2 = k1_mic1
        file_name = f"1k-{self.dump_files['1']['name']}"
        k1_ref = os.path.join(self.save_path, file_name)

        # silence
        self.send_command("start")
        time.sleep(cmd_sleep)
        self.send_command("bg 4")
        time.sleep(play_sleep)
        self.send_command("stop")
        time.sleep(cmd_sleep)
        self.start_dump("0", "silence")
        time.sleep(dump_sleep)
        self.start_dump("1", "silence")
        time.sleep(dump_sleep)

        file_name = f"silence-{self.dump_files['0']['name']}"
        silence_mic1 = os.path.join(self.save_path, file_name)
        silence_mic2 = silence_mic1
        file_name = f"silence-{self.dump_files['1']['name']}"
        silence_ref = os.path.join(self.save_path, file_name)

        # test all
        tool = AudioTestTools(self.save_path, self.logger)
        tool.test_all(k1_mic1, k1_mic2, k1_ref,
                      white_mic1, white_mic2, white_ref,
                      silence_mic1, silence_mic2, silence_ref)

        pass
