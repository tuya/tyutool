# Serial Debug Stress Validation

This note describes how to validate the serial-debug high-throughput fixes on macOS/Linux with the repo-local stress source:

- Script: `scripts/serial-debug-stress.py`
- Purpose: generate sustained PTY traffic with optional filter keywords, ANSI lines, and long logical lines

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
