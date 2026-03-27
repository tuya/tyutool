#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Test GUI startup speed.

Measures the actual time from process start to window ready,
by only importing what the GUI needs.
"""

import time
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

total_start = time.perf_counter()


def measure(label, func):
    start = time.perf_counter()
    result = func()
    elapsed = time.perf_counter() - start
    print(f"  {label}: {elapsed:.3f}s")
    return result


print("=== GUI Startup Speed Test ===\n")
print("[Phase 1] Core GUI imports:")

measure("PySide6 core",
        lambda: (__import__("PySide6"),
                 __import__("PySide6.QtCore"),
                 __import__("PySide6.QtGui"),
                 __import__("PySide6.QtWidgets")))

print("\n[Phase 2] tyutool GUI module imports:")

measure("tyutool.gui.ui_main",
        lambda: __import__("tyutool.gui.ui_main", fromlist=["Ui_MainWindow"]))
measure("tyutool.gui.ui_logo",
        lambda: __import__("tyutool.gui.ui_logo", fromlist=["LOGO_ICON_BYTES"]))
measure("tyutool.gui.flash",
        lambda: __import__("tyutool.gui.flash", fromlist=["FlashGUI"]))
measure("tyutool.gui.serial",
        lambda: __import__("tyutool.gui.serial", fromlist=["SerialGUI"]))
measure("tyutool.gui.ser_debug",
        lambda: __import__("tyutool.gui.ser_debug",
                           fromlist=["SerDebugGUI"]))
measure("tyutool.gui.web_debug",
        lambda: __import__("tyutool.gui.web_debug",
                           fromlist=["WebDebugGUI"]))
measure("tyutool.util",
        lambda: __import__("tyutool.util", fromlist=["TyutoolUpgrade"]))

total_import_time = time.perf_counter() - total_start
print(f"\n  Total import time: {total_import_time:.3f}s")

print(f"\n[Phase 3] GUI widget creation:")

from PySide6 import QtWidgets
app = QtWidgets.QApplication([])

from tyutool.gui.main import MyWidget
widget = measure("Create MyWidget", lambda: MyWidget())

total_time = time.perf_counter() - total_start

print(f"\n[Phase 4] Check deferred imports NOT loaded:")
heavy_modules = ["numpy", "scipy", "scipy.signal", "scipy.fft",
                 "matplotlib", "matplotlib.pyplot"]
for mod in heavy_modules:
    loaded = mod in sys.modules
    status = "LOADED (bad)" if loaded else "not loaded (good)"
    print(f"  {mod}: {status}")

print(f"\n{'='*40}")
print(f"  Total startup time: {total_time:.3f}s")
print(f"{'='*40}")

app.quit()
