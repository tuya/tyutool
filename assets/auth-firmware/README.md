# Default Authorization Firmware (auth-firmware)

Source firmware directory for the batch-flash-auth tool's "default authorization
firmware" list. Maintainers drop firmware bins here, then manually trigger
`.github/workflows/release-auth-firmware.yml` to generate `auth-firmware.json`
and publish both to the `auth-firmware` release on GitHub and Gitee.

## Directory layout and naming rules

```
assets/auth-firmware/
  <chip>/
    auth-firmware-<chip>-<version>.bin     # firmware
    auth-firmware-<chip>-<version>.txt     # optional: release notes (plain text)
```

Example:

```
assets/auth-firmware/
  esp32/
    auth-firmware-esp32-v1.0.0.bin
    auth-firmware-esp32-v1.1.0.bin
    auth-firmware-esp32-v1.1.0.txt
  bk7231n/
    auth-firmware-bk7231n-v1.0.0.bin
```

Rules (violations fail the release workflow):

1. The first-level directory name is the `chip` id — lowercase, alphanumeric
   (plus `_`), and must match a `ChipId` key in
   `src/features/firmware-flash/chip-manifests.ts`. This is the authoritative
   source of truth.
2. Each bin filename must be `auth-firmware-<chip>-<version>.bin`, where
   `<chip>` matches the parent directory name exactly.
3. `<version>` is whatever remains after stripping the `auth-firmware-<chip>-`
   prefix and the `.bin` suffix (e.g. `v1.1.0`).
4. If a sibling `auth-firmware-<chip>-<version>.txt` exists, its trimmed
   contents are used as `notes` for that version; otherwise `notes` is
   omitted.
5. **Do not create an `other/` directory.** `other` is the auth-only chip
   that runs through `FlashMode::Authorize` and has no default firmware.
6. Bins must live exactly two levels deep (`<source>/<chip>/<file>.bin`);
   nested subdirectories under a chip dir are ignored by both the manifest
   generator and the release uploader.

## Publishing

Manually dispatch **Release auth-firmware** (`workflow_dispatch`) from the
GitHub Actions page. The workflow:

- Runs `scripts/generate-auth-firmware-manifest.ts` to scan this directory,
  compute sha256/size, build the download URL, and write the manifest.
- Uploads to the `auth-firmware` release on GitHub and Gitee. **Existing bins
  are skipped (firmware is immutable per version); `auth-firmware.json` is
  always overwritten.**

Once published, a firmware version is immutable. To ship a new version, drop
a bin with a new `<version>` — never modify a published bin.
