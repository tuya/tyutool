#!/usr/bin/env python
# -*- coding: utf-8 -*-

import os
import sys
from datetime import datetime
from openpyxl import load_workbook


class AuthExcelParser:
    """Read and write TuyaOpen authorization Excel files (.xlsx).

    Expected columns (row 1 is header):
        A: UUID, B: AUTHKEY, C: STATUS, D: MAC, E: TIMESTAMP
    """

    COL_UUID = 1
    COL_AUTHKEY = 2
    COL_STATUS = 3
    COL_MAC = 4
    COL_TIMESTAMP = 5
    STATUS_USED = "USED"

    def __init__(self):
        self.filepath = None
        self.wb = None
        self.ws = None
        self._lock_fd = None

    def load(self, filepath):
        """Load an Excel file and parse authorization codes."""
        self.filepath = filepath
        self.wb = load_workbook(filepath)
        self.ws = self.wb.active

    def _ensure_header(self):
        headers = ["UUID", "AUTHKEY", "STATUS", "MAC", "TIMESTAMP"]
        for col, name in enumerate(headers, start=1):
            cell = self.ws.cell(row=1, column=col)
            if cell.value is None or str(cell.value).strip() == "":
                cell.value = name

    def get_stats(self):
        """Return (total, used, remain) counts."""
        total = 0
        used = 0
        for row in range(2, self.ws.max_row + 1):
            uuid_val = self.ws.cell(row=row, column=self.COL_UUID).value
            if not uuid_val:
                continue
            total += 1
            status = self.ws.cell(row=row, column=self.COL_STATUS).value
            if status and str(status).strip().upper() == self.STATUS_USED:
                used += 1
        return (total, used, total - used)

    def get_next_available(self):
        """Return (row_number, uuid, authkey) of the next unused code, or None."""
        for row in range(2, self.ws.max_row + 1):
            uuid_val = self.ws.cell(row=row, column=self.COL_UUID).value
            authkey_val = self.ws.cell(row=row, column=self.COL_AUTHKEY).value
            if not uuid_val or not authkey_val:
                continue
            status = self.ws.cell(row=row, column=self.COL_STATUS).value
            if status and str(status).strip().upper() == self.STATUS_USED:
                continue
            return (row, str(uuid_val).strip(), str(authkey_val).strip())
        return None

    def mark_used(self, row, mac, timestamp=None):
        """Mark a row as USED with MAC and timestamp, then save."""
        if timestamp is None:
            timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        self._ensure_header()
        self.ws.cell(row=row, column=self.COL_STATUS, value=self.STATUS_USED)
        self.ws.cell(row=row, column=self.COL_MAC, value=mac)
        self.ws.cell(row=row, column=self.COL_TIMESTAMP, value=timestamp)
        self.wb.save(self.filepath)

    def lock(self):
        """Acquire an advisory file lock to prevent concurrent writes."""
        lock_path = self.filepath + ".lock"
        self._lock_fd = open(lock_path, 'w')
        try:
            if sys.platform == 'win32':
                import msvcrt
                msvcrt.locking(self._lock_fd.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(self._lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return True
        except (IOError, OSError):
            self._lock_fd.close()
            self._lock_fd = None
            return False

    def unlock(self):
        """Release the advisory file lock."""
        if self._lock_fd:
            try:
                if sys.platform == 'win32':
                    import msvcrt
                    msvcrt.locking(self._lock_fd.fileno(), msvcrt.LK_UNLCK, 1)
                else:
                    import fcntl
                    fcntl.flock(self._lock_fd, fcntl.LOCK_UN)
                self._lock_fd.close()
            except Exception:
                pass
            self._lock_fd = None
            lock_path = self.filepath + ".lock"
            try:
                os.remove(lock_path)
            except OSError:
                pass

    def close(self):
        """Close workbook and release lock."""
        self.unlock()
        if self.wb:
            try:
                self.wb.close()
            except Exception:
                pass
            self.wb = None
            self.ws = None
