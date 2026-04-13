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
_PLACEHOLDER_UUID = "uuidxxxxxxxxxxxxxxxx"


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

    def debug(self, msg):
        """Discard device serial noise — not written to file or UI."""
        pass

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
        on_step(step_id, state, detail) — workflow step state change
    """

    STEP_OPEN = "open_serial"
    STEP_FLASH = "flash_firmware"
    STEP_RESET = "device_reset"
    STEP_READ_MAC = "read_mac"
    STEP_AUTH = "auth_write"
    STEP_VERIFY = "auth_verify"
    STEP_CLOSE = "close_serial"

    def __init__(self):
        self.excel = None
        self.ser = None
        self.protocol = None
        self.auth_log = None
        self._stop = False

        self.on_log = None
        self.on_device_info = None
        self.on_stats = None
        self.on_step = None

    def stop(self):
        self._stop = True

    def _log_callback(self, level, line):
        if self.on_log:
            self.on_log(level, line)

    def _emit_step(self, step_id, state, detail=""):
        if self.on_step:
            self.on_step(step_id, state, detail)

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

    def init_log(self, excel_path):
        """Create the session log file early (e.g. before flash).
        Returns the log file path."""
        log_path = self._build_log_path(excel_path)
        self.auth_log = AuthLogger(log_path, callback=self._log_callback)
        return log_path

    def authorize_single(self, port, baudrate, excel_path, skip_open=False):
        """Run single-device authorization. Returns True on success.

        Args:
            skip_open: When True, skip the STEP_OPEN emission (caller already
                       verified the port, e.g. before firmware flash).
        """
        self._stop = False
        own_log = False
        if not self.auth_log:
            self.init_log(excel_path)
            own_log = True

        try:
            if not self.excel:
                self.load_excel(excel_path)

            total, used, remain = self.excel.get_stats()
            self.auth_log.info(
                f"Loaded Excel: {excel_path}, total: {total}, available: {remain}")
            if self.on_stats:
                self.on_stats(total, used, remain)

            if not self.excel.lock():
                self.auth_log.error("Failed to acquire Excel file lock, another instance may be running")
                return False

            try:
                return self._do_authorize(port, baudrate, skip_open=skip_open)
            finally:
                self.excel.unlock()
                self._close_serial()
        finally:
            if own_log and self.auth_log:
                self.auth_log.close()
                self.auth_log = None

    def _open_serial(self, port, baudrate):
        self.auth_log.info(f"Opening serial: {port}, baudrate: {baudrate}")
        self.ser = serial.Serial()
        self.ser.port = port
        self.ser.baudrate = baudrate
        self.ser.timeout = 1
        self.ser.dtr = False
        self.ser.rts = False
        self.ser.open()
        self._drain_boot_output()
        self.ser.reset_input_buffer()
        self.protocol = AuthProtocol(self.ser, logger=self.auth_log)
        self.auth_log.info("Serial port ready")

    def _drain_boot_output(self, quiet_period=0.8, max_wait=5.0):
        """Read and discard data until the serial line is quiet, indicating
        that device boot is complete and the CLI shell is ready."""
        total_drained = 0
        last_data_time = time.time()
        deadline = time.time() + max_wait
        while time.time() < deadline:
            n = self.ser.in_waiting
            if n > 0:
                self.ser.read(n)
                total_drained += n
                last_data_time = time.time()
            else:
                if time.time() - last_data_time >= quiet_period:
                    break
                time.sleep(0.05)
        if total_drained > 0:
            self.auth_log.info(
                f"Drained {total_drained} bytes of boot output")

    def _close_serial(self):
        self._emit_step(self.STEP_CLOSE, "running")
        if self.ser and self.ser.is_open:
            self.ser.close()
        self.ser = None
        self.protocol = None
        self._emit_step(self.STEP_CLOSE, "done")

    def _do_authorize(self, port, baudrate, skip_open=False):
        # Step: open serial
        if not skip_open:
            self._emit_step(self.STEP_OPEN, "running")
        try:
            self._open_serial(port, baudrate)
        except Exception as e:
            self.auth_log.error(f"Failed to open serial: {e}")
            if not skip_open:
                self._emit_step(self.STEP_OPEN, "failed", str(e))
            if self.on_device_info:
                self.on_device_info("--", "--", "failed")
            return False
        if not skip_open:
            self._emit_step(self.STEP_OPEN, "done")

        if self._stop:
            return False

        # Step: read MAC
        self._emit_step(self.STEP_READ_MAC, "running")
        mac = self.protocol.read_mac()
        if not mac:
            self.auth_log.error("Failed to read MAC: device not responding")
            self._emit_step(self.STEP_READ_MAC, "failed", "Device not responding")
            if self.on_device_info:
                self.on_device_info("--", "--", "failed")
            return False
        self.auth_log.info(f"Device MAC: {mac}")
        self._emit_step(self.STEP_READ_MAC, "done", mac)
        if self.on_device_info:
            self.on_device_info(mac, "--", "reading")

        if self._stop:
            return False

        existing = self.protocol.auth_read()
        if existing:
            uuid_e, authkey_e = existing
            if uuid_e == _PLACEHOLDER_UUID:
                self.auth_log.info(
                    f"Found placeholder UUID={uuid_e}, treating as unauthorized")
                existing = None

        if existing:
            uuid_e, authkey_e = existing
            self.auth_log.info(
                f"Device already authorized, UUID={uuid_e}, AUTHKEY={mask_authkey(authkey_e)}")
            self._emit_step(self.STEP_AUTH, "done", "Already authorized")
            self._emit_step(self.STEP_VERIFY, "done", "Skipped")
            if self.on_device_info:
                self.on_device_info(mac, uuid_e, "already_authorized")
            row = self.excel.find_by_uuid(uuid_e)
            if row:
                ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
                self.excel.mark_used(row, mac, ts)
                self.auth_log.info(
                    f"Excel marked: UUID={uuid_e}, MAC={mac}")
                if self.on_stats:
                    self.on_stats(*self.excel.get_stats())
            else:
                self.auth_log.info(
                    f"UUID={uuid_e} not found in Excel, skipping mark")
            return True

        self.auth_log.info("Device not authorized, starting write")

        if self._stop:
            return False

        entry = self.excel.get_next_available()
        if not entry:
            self.auth_log.error("No authorization codes left")
            self._emit_step(self.STEP_AUTH, "failed", "No codes left")
            if self.on_device_info:
                self.on_device_info(mac, "--", "no_codes")
            return False

        row, uuid, authkey = entry
        self.auth_log.info(
            f"Assigning auth code: UUID={uuid}, AUTHKEY={mask_authkey(authkey)}")
        if self.on_device_info:
            self.on_device_info(mac, uuid, "writing")

        # Step: auth write
        self._emit_step(self.STEP_AUTH, "running")
        write_ok = False
        for attempt in range(1, MAX_RETRIES + 1):
            if self._stop:
                return False
            if self.protocol.auth_write(uuid, authkey):
                write_ok = True
                break
            self.auth_log.error(f"Write failed (attempt {attempt}/{MAX_RETRIES})")
            time.sleep(0.5)

        if not write_ok:
            self.auth_log.error("Write auth code failed, max retries reached")
            self._emit_step(self.STEP_AUTH, "failed", "Max retries reached")
            if self.on_device_info:
                self.on_device_info(mac, uuid, "write_failed")
            return False
        self._emit_step(self.STEP_AUTH, "done")

        if self._stop:
            return False

        # Step: auth verify
        self._emit_step(self.STEP_VERIFY, "running")
        readback = self.protocol.auth_read()
        if readback:
            rb_uuid, rb_authkey = readback
            self.auth_log.info(
                f"Readback verify: {rb_uuid} / {mask_authkey(rb_authkey)}")
            if rb_uuid == uuid and rb_authkey == authkey:
                self.auth_log.info("Readback verification passed")
                ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
                self.excel.mark_used(row, mac, ts)
                self.auth_log.info(
                    f"Authorization success: MAC={mac}, UUID={uuid}")
                self._emit_step(self.STEP_VERIFY, "done", "Passed")
                if self.on_device_info:
                    self.on_device_info(mac, uuid, "success")
                if self.on_stats:
                    self.on_stats(*self.excel.get_stats())
                return True
            else:
                self.auth_log.error(
                    f"Readback verification failed: wrote={uuid}/{mask_authkey(authkey)}, "
                    f"read={rb_uuid}/{mask_authkey(rb_authkey)}")
                self._emit_step(self.STEP_VERIFY, "failed", "Mismatch")
                if self.on_device_info:
                    self.on_device_info(mac, uuid, "verify_failed")
                return False
        else:
            self.auth_log.error("Readback failed: no response")
            self._emit_step(self.STEP_VERIFY, "failed", "No response")
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
            self.auth_log.error(f"Read MAC exception: {e}")
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
