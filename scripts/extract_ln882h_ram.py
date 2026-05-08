#!/usr/bin/env python3
import ast, sys, pathlib

if len(sys.argv) != 3:
    sys.exit(f"Usage: {sys.argv[0]} <ram_bin.py> <output.bin>")

# Source: https://raw.githubusercontent.com/tuya/tyutool/master/tyutool/flash/ln882h/ram_bin.py
src = pathlib.Path(sys.argv[1]).read_text()
data = None
for line in src.splitlines():
    if line.startswith('RAM_BIN'):
        data = ast.literal_eval(line.split('=', 1)[1].strip())
        break

if data is None:
    sys.exit(f"Error: no 'RAM_BIN = ...' assignment found in {sys.argv[1]}")

out = pathlib.Path(sys.argv[2])
out.write_bytes(data)
print(f"Wrote {len(data)} bytes to {out}")
