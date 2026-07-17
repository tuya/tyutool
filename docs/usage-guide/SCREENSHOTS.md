# Screenshot Capture Runbook

This runbook lists every screenshot the tyutool usage guide needs. A human
captures the PNGs by following the table below; the guide ships with styled
placeholders until each PNG lands.

The HTML pages already contain `<figure class="shot todo">` placeholder blocks
where each screenshot should appear. Once a PNG exists with the exact filename
below, replace the placeholder block with a real `figure.shot` (see
[Placeholder convention](#placeholder-convention)).

## Screenshot list

| Filename | Page | App location / route | Required UI state | Highlight / notes |
|---|---|---|---|---|
| `getting-started-connect.png` | Getting Started | `/flash` | Serial-port dropdown open, ≥1 port listed, status dot connected | Crop to connection bar |
| `flash-connection-bar.png` | Flash | `/flash` | Idle, chip + baud filled | Connection bar region |
| `flash-flash-tab.png` | Flash | `/flash` Flash tab | 2 segments loaded | Show multi-segment table |
| `flash-erase-tab.png` | Flash | `/flash` Erase tab | Advanced presets expanded | Show preset list |
| `flash-read-tab.png` | Flash | `/flash` Read tab | Dir + filename filled | — |
| `flash-authorize-tab.png` | Flash | `/flash` Authorize tab | UUID/AuthKey masked | Mask any real creds |
| `flash-progress-log.png` | Flash | `/flash` mid-operation | ~50% progress, log scrolling | Phase-colored bar |
| `serial-debug-overview.png` | Serial Debug | `/serial-debug` | Connected, streaming log | Full page |
| `serial-debug-hex-view.png` | Serial Debug | `/serial-debug` Hex view | 16 bytes/row | — |
| `serial-debug-filter-tabs.png` | Serial Debug | `/serial-debug` | 2 filter tabs with counts | — |
| `serial-debug-send-bar.png` | Serial Debug | `/serial-debug` send bar | ASCII mode, history open | — |
| `settings-update.png` | Settings | `/settings` | Update center banner | — |
| `settings-diagnostics.png` | Settings | `/settings` | Diagnostics section, log level visible | — |
| `batch-overview.png` ✓ | Batch Auth | `/toolbox/batch-flash-auth` | Several slots, mixed statuses | Captured; wired into operator page (zh + en) |
| `batch-config.png` | Batch Auth | config panel | Chip=esp32, firmware=Default | Collapsed dashboard ok |
| `batch-dashboard.png` | Batch Auth | dashboard | Mid-batch, donuts populated | — |
| `batch-slot-row.png` | Batch Auth | slot list | One row mid-flash, one done | Show status badges |
| `cli-terminal.png` | CLI | terminal | Successful `write`, rich mode | Optional |

That is 18 rows, including the optional `cli-terminal.png`.

## Capture guidance

- **Window size:** capture at a consistent **1280×800** browser window (or the
  app's default window if the Electron app is being shot). Keep the same size
  across all shots so the guide looks uniform.
- **Theme:** use the **light theme** by default. Dark-theme variants are
  optional — only capture them if time permits, and save them as
  `<filename>-dark.png` (they are not referenced by the HTML yet; they are for
  future use).
- **Privacy:** **hide or blur any real device IDs, UUIDs, AuthKeys, COM port
  names tied to a real device, and serial numbers.** Use dummy/test devices
  where possible. The Authorize-tab shot in particular must show masked
  UUID/AuthKey fields.
- **Format:** **PNG** (lossless, sharp text). No JPEG.
- **Save location:** save every PNG to `docs/usage-guide/assets/images/` using
  the **exact filename** from the table (case-sensitive). The HTML references
  them as `../assets/images/<filename>.png` from inside `zh/` and `en/`.
- **Cropping:** crop to the region noted in "Highlight / notes" when given
  (e.g. connection bar only); otherwise capture the full page region that
  illustrates the UI state. Leave a small margin so the screenshot breathes.
- **UI state:** set up the exact state in the "Required UI state" column before
  capturing. If a state is transient (e.g. "~50% progress"), start the
  operation and capture mid-flight; it is fine to retry until you get a clean
  frame.
- **Language:** capture against whichever language the screenshot is for; the
  same PNG is reused by both `zh/` and `en/` pages (the figure `alt` text
  differs per language, but the image file is shared).

## Placeholder convention

In the HTML, a missing screenshot renders as a styled placeholder:

```html
<figure class="shot todo">
  <figcaption>…per-language description of what the screenshot should show…</figcaption>
  <!-- capture spec: <filename>.png — …human note about the state to capture… -->
</figure>
```

Browsers render this as a dashed box reading "📷 待补截图 / Screenshot pending"
followed by the figcaption.

Once the matching PNG has been captured and saved to
`docs/usage-guide/assets/images/<filename>.png`, **replace** the placeholder
block with a real figure in **both** the `zh/` and `en/` copies of the page:

```html
<figure class="shot">
  <img src="../assets/images/<filename>.png" alt="…short alt text in the page's language…">
  <figcaption>…caption in the page's language…</figcaption>
</figure>
```

Notes:
- Drop the `todo` class from `figure` — that switches the box from the dashed
  placeholder to the bordered image style.
- Keep the `figcaption` text (translate per language) — it doubles as alt-text
  context.
- The `<!-- capture spec: … -->` comment can be removed once the real image is
  in place.
- The same PNG file is referenced by both `zh/` and `en/` pages, so capture
  once and wire it into both.
