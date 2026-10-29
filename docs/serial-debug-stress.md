# Serial Debug Stress Validation

This note describes how to validate the serial-debug high-throughput fixes on macOS/Linux with the repo-local stress source:

- Script: `scripts/serial-debug-stress.py`
- Purpose: generate sustained PTY traffic with optional filter keywords, ANSI lines, and long logical lines

## Regression acceptance covered by software tests

- An unterminated RX/TX prompt or binary response appears in the All view after
  the next 16 ms chunk batch. The preview updates as bytes arrive, carries the
  exact `rawBytes` used by Hex view, and is kept separate from completed lines:
  it is not archived, auto-saved, filtered, or appended to paged export. A later
  newline completes one line without duplicating the preview bytes. Pagination
  and export continue to use the completed archive snapshot.
- Newly archived TX/RX lines carry their original bytes through history and
  filter pages, including invalid UTF-8. Older text-only archive entries remain
  readable, but their original bytes cannot be reconstructed; Hex keeps the
  existing text fallback for those entries.
- The archive cap accounts for the JSON encoding overhead of the added
  `rawBytes` array as well as the serialized line. A cap test must therefore
  use the actual serialized size, not just text length or raw byte count.
- Automated frontend tests cover these boundaries with deterministic store,
  renderer, transport, and component fixtures. They do not constitute a
  sustained physical-device run. The stress scenarios below remain useful for
  software-level responsiveness and memory observation; no long-duration
  real-device stress test has been performed for this change.

## Auto-save overload behavior

The store enforces an 8 Mi `estimatedChars` budget (an estimated character
count, not 8 MiB of RAM) and a 32768-line cap on accepted pending auto-save
entries. Reaching either limit sets auto-save off and
gates subsequent queue insertion. One `serialDebug.autoSave.queueOverflow`
notice tells the user that auto-save stopped, the file may be incomplete, and
live logging continues. Already accepted entries remain queued and drain to the
original session file when writes resume.

The composable coalesces pressure-triggered flushing at 128 Ki estimated
characters or 256 queued lines into one macrotask. A non-draining turn writes at
most 16 batches, yields, then continues the captured sequence without waiting
for the next five-second interval. Deterministic tests currently verify a
50,000-short-line burst is written exactly once and a stalled writer reaches
the queue cap, emits one notice, and drains all accepted lines to the old path.
Clear-boundary ready-timer cancellation is also covered by the deterministic
regression tests. These software simulations do not establish real-device or
long-duration hardware behavior; no real-device stress test was performed.

## Quick Start

1. Run:

```bash
python3 scripts/serial-debug-stress.py
```

2. Copy the printed PTY slave path, for example `/dev/ttys012`.
3. Open that path in tyutool Serial Debug.
4. Let it run for a few minutes and observe:
   - the page remains responsive
   - memory does not grow without bound
   - log display continues updating
   - close / navigation / auto-save still complete normally

## Recommended Scenarios

Baseline throughput:

```bash
python3 scripts/serial-debug-stress.py --rate-kib 1024
```

Filter validation across the archived session:

```bash
python3 scripts/serial-debug-stress.py --rate-kib 1024 --keyword-every 8
```

Long-line / pending-buffer pressure:

```bash
python3 scripts/serial-debug-stress.py --rate-kib 1024 --extend-line-every 25
```

ANSI rendering pressure:

```bash
python3 scripts/serial-debug-stress.py --rate-kib 1024 --ansi-every 6
```

## What To Check

- Add a filter after the session has already accumulated history, and confirm old matching lines can still be loaded.
- Enable auto-save, let the session accumulate backlog, then close the port or leave the page and confirm the app does not freeze.
- Scroll the live view and confirm only a bounded visible window is retained in the page.
- Watch Activity Monitor during sustained traffic and confirm memory stabilizes instead of climbing continuously.
