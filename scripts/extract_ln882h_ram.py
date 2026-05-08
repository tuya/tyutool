#!/usr/bin/env python3
import sys, pathlib

src = pathlib.Path(sys.argv[1]).read_text()
# Find the assignment line and eval it
for line in src.splitlines():
    if line.startswith('RAM_BIN'):
        data = eval(line.split('=', 1)[1].strip())
        break

out = pathlib.Path(sys.argv[2])
out.write_bytes(data)
print(f"Wrote {len(data)} bytes to {out}")
