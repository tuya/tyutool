#!/usr/bin/env python3
"""
High-throughput PTY source for validating tyutool serial-debug behavior.

This script is intended for macOS/Linux manual validation:

1. Run this script locally. It will print a PTY slave path such as `/dev/ttys012`.
2. Open that slave path in tyutool's serial-debug page.
3. Let the script stream sustained traffic and observe:
   - UI remains responsive
   - memory does not grow without bound
   - filters still work across the whole archived session
   - close / navigation / auto-save remain stable under load

Examples:
  python3 scripts/serial-debug-stress.py
  python3 scripts/serial-debug-stress.py --rate-kib 2048 --keyword-every 8
  python3 scripts/serial-debug-stress.py --extend-line-every 25 --ansi-every 10
"""

from __future__ import annotations

import argparse
import os
import signal
import sys
import time


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Emit sustained serial-like traffic over a pseudo terminal."
    )
    parser.add_argument(
        "--rate-kib",
        type=int,
        default=512,
        help="Target throughput in KiB/s. Default: 512",
    )
    parser.add_argument(
        "--interval-ms",
        type=int,
        default=20,
        help="Write cadence in milliseconds. Default: 20",
    )
    parser.add_argument(
        "--duration-seconds",
        type=float,
        default=0.0,
        help="Stop after N seconds. 0 means run until Ctrl+C. Default: 0",
    )
    parser.add_argument(
        "--line-bytes",
        type=int,
        default=160,
        help="Approximate payload bytes per logical line. Default: 160",
    )
    parser.add_argument(
        "--keyword",
        default="FILTER_ME",
        help="Keyword periodically injected for whole-session filter validation.",
    )
    parser.add_argument(
        "--keyword-every",
        type=int,
        default=12,
        help="Inject keyword every Nth line. 0 disables. Default: 12",
    )
    parser.add_argument(
        "--ansi-every",
        type=int,
        default=16,
        help="Emit ANSI-colored lines every Nth line. 0 disables. Default: 16",
    )
    parser.add_argument(
        "--extend-line-every",
        type=int,
        default=0,
        help=(
            "Skip the trailing newline every Nth line to simulate long pending "
            "line growth. 0 disables. Default: 0"
        ),
    )
    parser.add_argument(
        "--header-every",
        type=int,
        default=64,
        help="Emit a visible section header every Nth line. 0 disables. Default: 64",
    )
    return parser


def make_line(seq: int, args: argparse.Namespace) -> bytes:
    timestamp = time.strftime("%H:%M:%S")
    base = f"[{timestamp}] seq={seq:08d} "

    if args.header_every > 0 and seq % args.header_every == 0:
        base += "==== BURST MARKER ===="
    elif args.keyword_every > 0 and seq % args.keyword_every == 0:
        base += f"{args.keyword} matched-line"
    else:
        base += "payload"

    if args.ansi_every > 0 and seq % args.ansi_every == 0:
        base = f"\x1b[32m{base}\x1b[0m"

    if len(base) < args.line_bytes:
        padding = "x" * (args.line_bytes - len(base))
        base = f"{base} {padding}"

    if args.extend_line_every > 0 and seq % args.extend_line_every == 0:
        return base.encode("utf-8", "replace")
    return (base + "\n").encode("utf-8", "replace")


def run(args: argparse.Namespace) -> int:
    if os.name != "posix":
        print("This script currently supports only macOS/Linux (POSIX PTY).", file=sys.stderr)
        return 2

    import pty

    master_fd, slave_fd = pty.openpty()
    slave_path = os.ttyname(slave_fd)

    stop = False

    def handle_stop(_signum, _frame) -> None:
        nonlocal stop
        stop = True

    signal.signal(signal.SIGINT, handle_stop)
    signal.signal(signal.SIGTERM, handle_stop)

    bytes_per_tick = max(1, int(args.rate_kib * 1024 * args.interval_ms / 1000))
    started_at = time.monotonic()
    next_tick = started_at
    seq = 1
    total_bytes = 0

    print(f"PTY slave path: {slave_path}")
    print(
        "Open this path in tyutool serial-debug, then observe responsiveness, "
        "memory, filtering, and close/auto-save behavior."
    )
    print(
        "Config: "
        f"rate={args.rate_kib}KiB/s interval={args.interval_ms}ms "
        f"line-bytes={args.line_bytes} keyword-every={args.keyword_every} "
        f"ansi-every={args.ansi_every} extend-line-every={args.extend_line_every}"
    )
    print("Press Ctrl+C to stop.")
    sys.stdout.flush()

    try:
        while not stop:
            if args.duration_seconds > 0 and time.monotonic() - started_at >= args.duration_seconds:
                break

            burst = bytearray()
            while len(burst) < bytes_per_tick:
                burst.extend(make_line(seq, args))
                seq += 1

            os.write(master_fd, burst)
            total_bytes += len(burst)

            next_tick += args.interval_ms / 1000.0
            sleep_for = next_tick - time.monotonic()
            if sleep_for > 0:
                time.sleep(sleep_for)
            else:
                # The generator fell behind; reset cadence so it does not drift forever.
                next_tick = time.monotonic()
    finally:
        os.close(master_fd)
        os.close(slave_fd)
        elapsed = max(0.001, time.monotonic() - started_at)
        print(
            f"Stopped after {elapsed:.2f}s, wrote {total_bytes} bytes "
            f"({total_bytes / elapsed / 1024:.1f} KiB/s actual)."
        )

    return 0


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    return run(args)


if __name__ == "__main__":
    raise SystemExit(main())
