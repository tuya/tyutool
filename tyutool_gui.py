#!/usr/bin/env python
# -*- coding: utf-8 -*-

import sys
import os
import traceback

# Debug log file next to the executable
_debug_log = os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])),
                          "tyutool_debug.log")
_debug_f = open(_debug_log, 'w')


def _dbg(msg):
    _debug_f.write(msg + '\n')
    _debug_f.flush()


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
    _debug_f.close()
