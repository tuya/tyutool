#!/usr/bin/env python
# -*- coding: utf-8 -*-

import re
import time

CMD_TIMEOUT = 3.0
IDLE_TIMEOUT = 0.3
AUTH_WRITE_SUCCESS = "Authorization write succeeds."
MAC_PATTERN = re.compile(r'([0-9A-Fa-f]{2}(?::[0-9A-Fa-f]{2}){5})')


class AuthProtocol:
    """Serial protocol for TuyaOpen device authorization commands."""

    def __init__(self, serial_port, logger=None):
        self.ser = serial_port
        self.logger = logger

    def _log(self, level, msg):
        if self.logger:
            getattr(self.logger, level)(msg)

    def _flush_input(self):
        self.ser.reset_input_buffer()

    def _send_cmd(self, cmd):
        self._flush_input()
        data = (cmd + "\r\n").encode('utf-8')
        self.ser.write(data)
        self.ser.flush()
        self._log("info", f">> {cmd}")

    def _read_response(self, timeout=CMD_TIMEOUT, idle_timeout=IDLE_TIMEOUT):
        """Read serial response lines within timeout.

        Returns early if no new data arrives for idle_timeout seconds
        after the first data is received.
        """
        buf = b''
        lines = []
        last_data_time = None
        end_time = time.time() + timeout
        while time.time() < end_time:
            n = self.ser.in_waiting
            if n > 0:
                buf += self.ser.read(n)
                last_data_time = time.time()
                while b'\n' in buf:
                    raw_line, buf = buf.split(b'\n', 1)
                    try:
                        line = raw_line.decode('utf-8', errors='replace').strip()
                    except Exception:
                        line = raw_line.decode('latin-1', errors='replace').strip()
                    if line:
                        lines.append(line)
                        self._log("info", f"<< {line}")
            else:
                if last_data_time and (time.time() - last_data_time > idle_timeout):
                    break
                time.sleep(0.05)
        if buf:
            trailing = buf.decode('utf-8', errors='replace').strip()
            if trailing:
                lines.append(trailing)
                self._log("info", f"<< {trailing}")
        return lines

    def read_mac(self):
        """Send read_mac command and return MAC address string or None."""
        self._send_cmd("read_mac")
        lines = self._read_response()
        for line in lines:
            m = MAC_PATTERN.search(line)
            if m:
                return m.group(1).upper()
        return None

    def auth_read(self):
        """Send auth-read command and return (uuid, authkey) or None."""
        self._send_cmd("auth-read")
        lines = self._read_response()
        non_empty = [l for l in lines if l and "auth-read" not in l.lower()]
        if len(non_empty) >= 2:
            uuid_val = non_empty[0].strip()
            authkey_val = non_empty[1].strip()
            if uuid_val and authkey_val:
                return (uuid_val, authkey_val)
        return None

    def auth_write(self, uuid, authkey):
        """Send auth write command and return True if success."""
        self._send_cmd(f"auth {uuid} {authkey}")
        lines = self._read_response()
        for line in lines:
            if AUTH_WRITE_SUCCESS in line:
                return True
        return False
