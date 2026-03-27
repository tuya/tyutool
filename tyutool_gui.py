#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import os
import traceback
import faulthandler

# Debug log file next to the executable
_debug_log = os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])),
                          "tyutool_debug.log")
try:
    _debug_f = open(_debug_log, 'w')
except Exception:
    _debug_f = None

# Enable faulthandler — write crash tracebacks to debug log (or stderr if available)
# sys.stderr is None in PyInstaller --windowed mode, so use the log file as fallback
_fault_file = _debug_f if _debug_f else sys.stderr
if _fault_file is not None:
    try:
        faulthandler.enable(file=_fault_file)
    except Exception:
        pass


def _dbg(msg):
    if _debug_f:
        try:
            _debug_f.write(msg + '\n')
            _debug_f.flush()
        except Exception:
            pass


_dbg(f"=== tyutool_gui starting ===")
_dbg(f"sys.argv: {sys.argv}")
_dbg(f"sys.executable: {sys.executable}")
_dbg(f"is_frozen: {getattr(sys, 'frozen', False)}")

try:
    _dbg("importing tyutool.gui ...")
    from tyutool.gui import gui
    _dbg("import done, calling gui() ...")
    gui()
except Exception as e:
    _dbg(f"EXCEPTION: {e}")
    _dbg(traceback.format_exc())
finally:
    _dbg("=== tyutool_gui exiting ===")
    if _debug_f:
        _debug_f.close()
