#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Test GUI thread lifecycle — ensures no 'QThread destroyed while running' crash.

Usage:
    QT_QPA_PLATFORM=offscreen python tests/test_thread_lifecycle.py
"""

import sys
import os
import time

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from PySide6 import QtWidgets, QtCore
from PySide6.QtCore import QTimer

PASS = 0
FAIL = 0


def report(name, ok, detail=""):
    global PASS, FAIL
    status = "PASS" if ok else "FAIL"
    if ok:
        PASS += 1
    else:
        FAIL += 1
    msg = f"  [{status}] {name}"
    if detail:
        msg += f" — {detail}"
    print(msg)


def process_events(duration_s=0.5):
    """Run Qt event loop for a given duration."""
    deadline = time.perf_counter() + duration_s
    while time.perf_counter() < deadline:
        app.processEvents()
        time.sleep(0.02)


app = QtWidgets.QApplication([])


# ---- Test 1: EmittingStr is thread-safe (no QEventLoop in write) ----
print("\n[Test 1] EmittingStr thread-safety")

from tyutool.gui.main import EmittingStr
from PySide6.QtCore import QThread, Signal


class WriterThread(QThread):
    """Writes to EmittingStr from a background thread."""
    done = Signal(bool)

    def __init__(self, stream):
        super().__init__()
        self.stream = stream

    def run(self):
        try:
            for i in range(5):
                self.stream.write(f"bg-thread msg {i}\n")
            self.done.emit(True)
        except Exception as e:
            print(f"    WriterThread exception: {e}")
            self.done.emit(False)


emitting = EmittingStr()
received = []
emitting.textWritten.connect(lambda t: received.append(t),
                             QtCore.Qt.DirectConnection)

writer = WriterThread(emitting)
writer_ok = [None]


def on_writer_done(ok):
    writer_ok[0] = ok


writer.done.connect(on_writer_done, QtCore.Qt.DirectConnection)
writer.start()
writer.wait(5000)

report("EmittingStr.write from bg thread doesn't crash",
       writer_ok[0] is True,
       f"received {len(received)} messages")


# ---- Test 2: AskUpgradeThread emits signal and completes ----
print("\n[Test 2] AskUpgradeThread lifecycle")

from tyutool.gui.main import AskUpgradeThread

ask_thread = AskUpgradeThread()
ask_result = [None]


def on_ask_result(should, version):
    ask_result[0] = (should, version)


ask_thread.should_upgrade.connect(on_ask_result, QtCore.Qt.DirectConnection)
ask_thread.start()
finished = ask_thread.wait(10000)  # 10s timeout for network

report("AskUpgradeThread finishes within timeout",
       finished, f"wait returned {finished}")
report("AskUpgradeThread emitted result signal",
       ask_result[0] is not None,
       f"result={ask_result[0]}")


# ---- Test 3: Widget closeEvent cleans up threads ----
print("\n[Test 3] MyWidget close cleans up threads")

from tyutool.gui.main import MyWidget

widget = MyWidget()
widget.show()

# Let the ask-upgrade thread start
QTimer.singleShot(200, widget.close)

# Run event loop briefly to process
deadline = time.perf_counter() + 8
while time.perf_counter() < deadline:
    app.processEvents()
    if not widget.isVisible():
        break
    time.sleep(0.05)

thread_alive = (widget.ask_upgrade_thread is not None
                and widget.ask_upgrade_thread.isRunning())
report("ask_upgrade_thread stopped after close",
       not thread_alive,
       f"thread ref={widget.ask_upgrade_thread}")

thread_alive2 = (widget.upgrade_thread is not None
                 and widget.upgrade_thread.isRunning())
report("upgrade_thread stopped after close",
       not thread_alive2)


# ---- Test 4: Chip selection doesn't crash ----
print("\n[Test 4] Chip selection (ESP32S3)")

widget2 = MyWidget()
widget2.show()
process_events(0.3)

# Wait for ask_upgrade_thread to finish
if widget2.ask_upgrade_thread:
    widget2.ask_upgrade_thread.wait(10000)

chip_crash = False
try:
    chips = ["ESP32S3", "BK7231N", "ESP32C3", "T5", "LN882H"]
    for chip in chips:
        print(f"    Selecting {chip}...")
        widget2.ui.comboBoxChip.setCurrentText(chip)
        process_events(0.3)
    # Wait for any pic_loader to finish
    if widget2.pic_loader and widget2.pic_loader.isRunning():
        widget2.pic_loader.wait(5000)
except Exception as e:
    chip_crash = True
    import traceback
    traceback.print_exc()

report("Chip selection no crash", not chip_crash)

widget2.close()
process_events(0.3)


# ---- Test 5: Rapid create-destroy doesn't crash ----
print("\n[Test 5] Rapid widget create/destroy")

crash = False
try:
    for i in range(3):
        w = MyWidget()
        w.show()
        app.processEvents()
        w.close()
        app.processEvents()
        # Must wait for threads before next iteration
        if w.ask_upgrade_thread:
            w.ask_upgrade_thread.wait(5000)
except Exception as e:
    crash = True
    print(f"    Exception: {e}")

report("Rapid create/destroy no crash", not crash)


# ---- Summary ----
print(f"\n{'='*40}")
print(f"  Results: {PASS} passed, {FAIL} failed")
print(f"{'='*40}")

app.quit()
sys.exit(1 if FAIL > 0 else 0)
