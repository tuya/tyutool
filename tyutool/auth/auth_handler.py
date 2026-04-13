#!/usr/bin/env python
# -*- coding: utf-8 -*-

import os
import time
from datetime import datetime

import serial

from .auth_protocol import AuthProtocol
from .excel_parser import AuthExcelParser

MAX_RETRIES = 3
DEFAULT_BAUD = 115200


def mask_authkey(authkey):
    """Return masked authkey for log output."""
    if len(authkey) <= 4:
        return "****"
    return authkey[:8] + "****"


class _NullLogger:
    """No-op logger used when auth_log is not initialized."""
    def info(self, msg): pass
    def error(self, msg): pass
    def debug(self, msg): pass
    def log(self, level, msg): pass
    def close(self): pass


class AuthLogger:
    """Dual-output logger: writes to both a callback and a log file."""

    def __init__(self, log_path, callback=None):
        self.log_path = log_path
        self.callback = callback
        self._f = open(log_path, 'a', encoding='utf-8')

    def log(self, level, msg):
        if not self._f:
            return
        ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        line = f"[{ts}] [{level}] {msg}"
        self._f.write(line + "\n")
        self._f.flush()
        if self.callback:
            self.callback(level, line)

    def info(self, msg):
        self.log("INFO", msg)

    def error(self, msg):
        self.log("ERROR", msg)

    def close(self):
        if self._f:
            self._f.close()
            self._f = None


class AuthHandler:
    """Core authorization logic: serial command exchange and flow control.

    Callbacks (all optional, set as attributes):
        on_log(level, line)       — log message
        on_device_info(mac, uuid, status) — current device info update
        on_stats(total, used, remain) — stats update
    """

    def __init__(self):
        self.excel = None
        self.ser = None
        self.protocol = None
        self.auth_log = None
        self._stop = False

        self.on_log = None
        self.on_device_info = None
        self.on_stats = None

    def stop(self):
        self._stop = True

    def _log_callback(self, level, line):
        if self.on_log:
            self.on_log(level, line)

    def load_excel(self, filepath):
        """Load Excel file and return (total, used, remain)."""
        self.excel = AuthExcelParser()
        self.excel.load(filepath)
        return self.excel.get_stats()

    def _build_log_path(self, excel_path):
        directory = os.path.dirname(excel_path)
        basename = os.path.splitext(os.path.basename(excel_path))[0]
        ts = datetime.now().strftime("%Y%m%d_%H%M%S")
        return os.path.join(directory, f"{basename}_auth_{ts}.log")

    def authorize_single(self, port, baudrate, excel_path):
        """Run single-device authorization. Returns True on success."""
        self._stop = False
        log_path = self._build_log_path(excel_path)
        self.auth_log = AuthLogger(log_path, callback=self._log_callback)

        try:
            if not self.excel:
                self.load_excel(excel_path)

            total, used, remain = self.excel.get_stats()
            self.auth_log.info(
                f"加载 Excel: {excel_path}, 总计: {total}, 可用: {remain}")
            if self.on_stats:
                self.on_stats(total, used, remain)

            if not self.excel.lock():
                self.auth_log.error("无法获取 Excel 文件锁，可能有其他实例正在运行")
                return False

            try:
                return self._do_authorize(port, baudrate)
            finally:
                self.excel.unlock()
                self._close_serial()
        finally:
            if self.auth_log:
                self.auth_log.close()
                self.auth_log = None

    def _open_serial(self, port, baudrate):
        self.auth_log.info(f"打开串口: {port}, 波特率: {baudrate}")
        self.ser = serial.Serial()
        self.ser.port = port
        self.ser.baudrate = baudrate
        self.ser.timeout = 1
        self.ser.dtr = False
        self.ser.rts = False
        self.ser.open()
        time.sleep(0.3)
        pending = self.ser.in_waiting
        if pending > 0:
            self.ser.read(pending)
            self.auth_log.info(f"清空启动残留数据: {pending} 字节")
        self.ser.reset_input_buffer()
        self.protocol = AuthProtocol(self.ser, logger=self.auth_log)
        self.auth_log.info("串口就绪")

    def _close_serial(self):
        if self.ser and self.ser.is_open:
            self.ser.close()
        self.ser = None
        self.protocol = None

    def _do_authorize(self, port, baudrate):
        self._open_serial(port, baudrate)

        if self._stop:
            return False

        mac = self.protocol.read_mac()
        if not mac:
            self.auth_log.error("读取 MAC 失败: 设备无响应")
            if self.on_device_info:
                self.on_device_info("--", "--", "failed")
            return False
        self.auth_log.info(f"设备 MAC: {mac}")
        if self.on_device_info:
            self.on_device_info(mac, "--", "reading")

        if self._stop:
            return False

        existing = self.protocol.auth_read()
        if existing:
            uuid_e, authkey_e = existing
            self.auth_log.info(
                f"设备已授权, UUID={uuid_e}, AUTHKEY={mask_authkey(authkey_e)}")
            if self.on_device_info:
                self.on_device_info(mac, uuid_e, "already_authorized")
            return True

        self.auth_log.info("设备未授权, 开始写入")

        if self._stop:
            return False

        entry = self.excel.get_next_available()
        if not entry:
            self.auth_log.error("授权码已用完")
            if self.on_device_info:
                self.on_device_info(mac, "--", "no_codes")
            return False

        row, uuid, authkey = entry
        self.auth_log.info(
            f"分配授权码: UUID={uuid}, AUTHKEY={mask_authkey(authkey)}")
        if self.on_device_info:
            self.on_device_info(mac, uuid, "writing")

        write_ok = False
        for attempt in range(1, MAX_RETRIES + 1):
            if self._stop:
                return False
            if self.protocol.auth_write(uuid, authkey):
                write_ok = True
                break
            self.auth_log.error(f"写入失败 (尝试 {attempt}/{MAX_RETRIES})")
            time.sleep(0.5)

        if not write_ok:
            self.auth_log.error("写入授权码失败，已达最大重试次数")
            if self.on_device_info:
                self.on_device_info(mac, uuid, "write_failed")
            return False

        if self._stop:
            return False

        readback = self.protocol.auth_read()
        if readback:
            rb_uuid, rb_authkey = readback
            self.auth_log.info(
                f"回读校验: {rb_uuid} / {mask_authkey(rb_authkey)}")
            if rb_uuid == uuid and rb_authkey == authkey:
                self.auth_log.info("回读校验通过")
                ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
                self.excel.mark_used(row, mac, ts)
                self.auth_log.info(
                    f"授权成功: MAC={mac}, UUID={uuid}")
                if self.on_device_info:
                    self.on_device_info(mac, uuid, "success")
                if self.on_stats:
                    self.on_stats(*self.excel.get_stats())
                return True
            else:
                self.auth_log.error(
                    f"回读校验失败: 写入={uuid}/{mask_authkey(authkey)}, "
                    f"回读={rb_uuid}/{mask_authkey(rb_authkey)}")
                if self.on_device_info:
                    self.on_device_info(mac, uuid, "verify_failed")
                return False
        else:
            self.auth_log.error("回读失败: 无响应")
            if self.on_device_info:
                self.on_device_info(mac, uuid, "verify_failed")
            return False

    def read_mac_only(self, port, baudrate):
        """Read device MAC without doing authorization."""
        if not self.auth_log:
            self.auth_log = _NullLogger()
            _cleanup_log = True
        else:
            _cleanup_log = False
        try:
            self._open_serial(port, baudrate)
            mac = self.protocol.read_mac()
            return mac
        except Exception as e:
            self.auth_log.error(f"读取 MAC 异常: {e}")
            return None
        finally:
            self._close_serial()
            if _cleanup_log:
                self.auth_log = None

    def cleanup(self):
        """Release all resources."""
        self._close_serial()
        if self.excel:
            self.excel.close()
            self.excel = None
        if self.auth_log:
            self.auth_log.close()
            self.auth_log = None
