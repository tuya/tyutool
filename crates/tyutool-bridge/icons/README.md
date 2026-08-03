# tyutool-bridge icons

App/installer icons for **Cobuilder Bridge**, generated from the product logo.

```bash
# from the repo root
pnpm exec tauri icon <path-to-logo.png> -o crates/tyutool-bridge/icons
# then delete the mobile/UWP output the desktop bridge has no use for:
rm -rf crates/tyutool-bridge/icons/{android,ios} \
       crates/tyutool-bridge/icons/{Square*Logo.png,StoreLogo.png}
```

Source image: the Cobuilder logo mark, 3326×3326 PNG with alpha. `tauri icon`
wants a square source with transparency and at least 1024×1024.

## Do not point these at `src-tauri/icons/`

That directory belongs to the **GUI desktop app** and is referenced by
`src-tauri/tauri.conf.json`. The two products have separate icon sets on purpose;
regenerating one must never overwrite the other.

## Who consumes what

| File | Consumer |
|---|---|
| `icon.icns` | the macOS `.app` bundle (Finder, the `.dmg` window, ⌘-Tab) |
| `icon.ico` | the Windows `.exe` resource (`build.rs`), and therefore both shortcuts, the taskbar and the "Programs and Features" entry |
| `128x128.png`, `128x128@2x.png`, `64x64.png`, `32x32.png` | Linux `hicolor` theme directories in the `.deb`/`.AppImage`; `cargo-packager` files each one by its real pixel size |

## Not the tray glyph

The menu-bar/status icon is **not** from this set — it is drawn in code
(`placeholder_icon` in `main.rs`) as a monochrome ring. macOS renders a status
item as a *template image*, recoloring it to match light/dark mode, which would
flatten this multi-colour logo into a solid blob. A proper tray glyph needs a
purpose-made single-colour design and is tracked separately.
