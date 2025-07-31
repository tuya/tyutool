#!/usr/bin/env python3
# coding=utf-8

import sys
import time
import serial
import threading

from tyutool.cli import choose_port


class SerAIDebugMonitor(object):
    def __init__(self, port, baud, save, logger):
        self.port = port or choose_port()
        self.baud = baud
        self.save = save

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
        """打开指定串口"""
        try:
            self.ser = serial.Serial(
                port=self.port,
                baudrate=2000000,
                timeout=0.1
            )
            time.sleep(0.5)  # 等待串口初始化
            return True
        except Exception as e:
            print(f"打开串口失败: {e}")
            return False

    def start_reading(self):
        """启动数据读取线程"""
        self.running = True
        self.read_thread = threading.Thread(target=self.read_from_serial)
        self.read_thread.daemon = True
        self.read_thread.start()

    def stop_reading(self):
        """停止数据读取"""
        self.running = False
        if hasattr(self, 'read_thread') and self.read_thread.is_alive():
            self.read_thread.join()

    def read_from_serial(self):
        """串口数据读取线程"""
        while self.running:
            if self.ser and self.ser.is_open:
                try:
                    if self.ser.in_waiting > 0:
                        data = self.ser.read(self.ser.in_waiting)
                        self.last_data_time = time.time()  # 更新最后接收时间
                        self.process_received_data(data)
                except Exception as e:
                    print(f"读取错误: {e}")

            # 检查dump超时
            if self.current_dump_channel:
                if time.time() - self.last_data_time > 0.1:  # 100ms超时
                    self.stop_dump(self.current_dump_channel)

            time.sleep(0.01)

    def process_received_data(self, data):
        """处理接收到的数据"""
        # 更新dump文件长度
        if self.current_dump_channel and self.dump_files[self.current_dump_channel]['active']:
            self.dump_files[self.current_dump_channel]['length'] += len(data)
            self.update_length_display()

        # 写入当前dump文件
        if self.current_dump_channel and self.dump_files[self.current_dump_channel]['file']:
            self.dump_files[self.current_dump_channel]['file'].write(data)

        # 非dump模式下显示接收数据
        if not self.current_dump_channel:
            try:
                print(data.decode('utf-8', errors='replace'), end='')
            except:
                pass  # 忽略非文本数据

    def update_length_display(self):
        """更新dump长度显示（同一行刷新）"""
        if self.current_dump_channel:
            channel = self.current_dump_channel
            length = self.dump_files[channel]['length']
            times = length/32000
            sys.stdout.write(f"\r接收数据长度: {length} 字节, 录音时间 {times:.3f} s")
            sys.stdout.flush()

    def send_command(self, cmd):
        """发送命令到串口"""
        if not self.ser or not self.ser.is_open:
            print("串口未连接！")
            return False

        try:
            # 添加"ao "前缀和回车换行
            full_cmd = f"ao {cmd}\r\n"
            self.ser.write(full_cmd.encode('utf-8'))
            print(f"已发送: ao {cmd}")
            return True
        except Exception as e:
            print(f"发送失败: {e}")
            return False

    def start_dump(self, channel):
        """启动数据转储"""
        if channel not in self.dump_files:
            print(f"无效通道: {channel}")
            return

        # 如果已有dump在运行，先停止
        if self.current_dump_channel:
            self.stop_dump(self.current_dump_channel)

        # 发送dump命令
        self.send_command(f"dump {channel}")

        try:
            # 打开文件（二进制写入），覆盖已存在的文件
            self.dump_files[channel]['file'] = open(self.dump_files[channel]['name'], 'wb')
            self.dump_files[channel]['active'] = True
            self.dump_files[channel]['length'] = 0
            self.current_dump_channel = channel
            self.last_data_time = time.time()  # 记录开始时间

            print(f"开始转储通道 {channel} -> {self.dump_files[channel]['name']}")
            print("等待接收数据...(100ms无数据自动停止)")
        except Exception as e:
            print(f"创建文件失败: {e}")

    def stop_dump(self, channel=None):
        """停止数据转储"""
        if not channel and self.current_dump_channel:
            channel = self.current_dump_channel

        if channel and channel in self.dump_files and self.dump_files[channel]['active']:
            # 停止指定通道
            if self.dump_files[channel]['file']:
                self.dump_files[channel]['file'].close()
            self.dump_files[channel]['file'] = None
            self.dump_files[channel]['active'] = False

            length = self.dump_files[channel]['length']
            print(f"\n已停止转储通道 {channel}, 接收长度: {length} 字节")
            print(f"数据已保存到: {self.dump_files[channel]['name']}")

            # 重置当前通道
            self.current_dump_channel = None

    def close_port(self):
        """关闭串口"""
        if self.current_dump_channel:
            self.stop_dump(self.current_dump_channel)

        if self.ser and self.ser.is_open:
            self.ser.close()
            print("串口已关闭")
