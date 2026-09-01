# RAM Loader Assets (ram-loader)

Source directory for the **RAM loaders** two chips cannot be flashed without:
their mask ROM can do little more than write SRAM and jump to it, so tyutool has
to upload a downloader *first* and let that program the flash.

| Chip | What it is | Uploaded to |
|---|---|---|
| `ln882h` | Vendor RAM code, XMODEM-uploaded, answers `RAMCODE` | `0x20000000` |
| `gd32vw553` | Vendor USART downloader, uploaded over the AN3155 ROM bootloader | `0x20002000` |

These used to be `include_bytes!`-compiled into `tyutool-core`
(`plugins/ln882h/ram.bin`, `plugins/gd32/loader.bin`). They are now published
assets, downloaded on demand and cached — the shipped binaries carry no vendor
firmware. Maintainers drop bins here, then manually trigger
`.github/workflows/release-ram-loader.yml` to generate `ram-loader.json` and
publish both to the `ram-loader` release on GitHub and Gitee.

**This is not the auth-firmware set.** `assets/auth-firmware/` holds authorization
firmware the *user* picks from a list; a RAM loader is mandatory infrastructure
for one chip, pinned by the code that uses it. Separate release tag, separate
manifest, separate cache directory.

## Directory layout and naming rules

```
assets/ram-loader/
  <chip>/
    ram-loader-<chip>-<version>.bin     # loader image
    ram-loader-<chip>-<version>.txt     # optional: provenance / notes (plain text)
```

Rules (violations fail the release workflow):

1. The first-level directory name is the `chip` id — lowercase, alphanumeric
   (plus `_`), and must match a `ChipId` key in
   `src/features/firmware-flash/chip-manifests.ts`.
2. Each bin filename must be `ram-loader-<chip>-<version>.bin`, where `<chip>`
   matches the parent directory name exactly.
3. `<version>` is whatever remains after stripping the `ram-loader-<chip>-`
   prefix and the `.bin` suffix (e.g. `1.0.0`).
4. If a sibling `ram-loader-<chip>-<version>.txt` exists, its trimmed contents
   are used as `notes` for that version; otherwise `notes` is omitted.
5. Bins must live exactly two levels deep (`<source>/<chip>/<file>.bin`); nested
   subdirectories under a chip dir are ignored by both the manifest generator
   and the release uploader.

`<version>` is **tyutool's own** asset version, not the vendor's. Neither vendor
ships a usable version number: the GD32 image leaves its own
`SDK release version:` banner empty (it carries only build revision
`94fb25571b15fbea` / `2025/07/04`), and the LN882H image carries no version
information whatsoever. Record whatever provenance the image does carry in the
`.txt` notes, and bump `<version>` monotonically here.

## Pinned in code, not "latest wins"

Each plugin declares the one loader it was written against, digest included:

```rust
// crates/tyutool-core/src/plugins/gd32/mod.rs
const LOADER: RamLoaderRef = RamLoaderRef { chip: "gd32vw553", version: "1.0.0", size: 15_600, sha256: "…" };
```

The runtime downloads *that* entry and verifies it against the digest compiled
into the binary — the manifest's own `sha256` is only a cross-check. So a rolled
or tampered manifest cannot feed a plugin a different loader, and publishing a
new loader version never changes what an already-shipped tool uploads.

Shipping a new loader therefore takes both halves:

1. Drop `ram-loader-<chip>-<new version>.bin` here (plus notes) and run the
   release workflow. Published versions are immutable — never modify a published
   bin.
2. Update that chip's `RamLoaderRef` (version + sha256 + size) and release a
   tool version. Until then the new asset simply sits unused.

## Publishing

Manually dispatch **Release ram-loader** (`workflow_dispatch`) from the GitHub
Actions page. The workflow:

- Runs `scripts/generate-ram-loader-manifest.ts` to scan this directory, compute
  sha256/size, build the download URL, and write `ram-loader.json`.
- Uploads to the `ram-loader` release on GitHub and Gitee. **Existing bins are
  skipped (a loader version is immutable); `ram-loader.json` is always
  overwritten.**

The Tuya CDN copy is uploaded by hand, flat, under
`smart/embed/pruduct/tyutool/ram-loader/`; regenerate the manifest with that
`BASE_URL` for it.
