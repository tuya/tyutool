# tyutool brand assets

The orange data block and dark receiving socket represent firmware being written
to a device. The selected mark uses a centered entrance and moderate socket depth
to suggest insertion while retaining a substantial base.

## Assets

| File                                                       | Purpose                                                                              |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| [tyutool-data-write-logo.svg](tyutool-data-write-logo.svg) | **Master artwork**: symbol and outlined wordmark; no external fonts or bitmap images |
| [tyutool-data-write-icon.svg](tyutool-data-write-icon.svg) | Standalone symbol, extracted from the master                                         |
| [tyutool-data-write-logo.png](tyutool-data-write-logo.png) | Full logo, 1024 × 1024, transparent background                                       |
| [tyutool-data-write-icon.png](tyutool-data-write-icon.png) | Standalone icon, 1024 × 1024, transparent background                                 |

Colors: orange `#FF793E`, dark ink `#263346`.

## Export

Edit `tyutool-data-write-logo.svg`, then run from the repository root with the
repository dependencies and Playwright Chromium installed:

```sh
pnpm exec tsx scripts/export-logo-assets.ts
```

The [export script](../scripts/export-logo-assets.ts) extracts the standalone
symbol and regenerates both PNGs in this directory. Keep the SVG master as the
source for all derived artwork.
