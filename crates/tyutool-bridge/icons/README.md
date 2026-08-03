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

## The tray glyph is a separate pair of assets

The menu-bar / notification-area icon does **not** come from the set above. It is
`tray-macos.png` + `tray-color.png`, embedded via `include_bytes!` and decoded at
startup by `src/tray_glyph.rs` (`tray_icon::Icon` only takes raw RGBA, and
`from_path` would mean shipping a loose file and resolving it inside the `.app`).

Two assets, because "works in light and dark" is a different mechanism per
platform and the wrong one is **invisible**, not merely ugly:

| Asset | Platform | Why |
|---|---|---|
| `tray-macos.png` (44 px, black + alpha) | macOS | A status item is a *template image*: macOS ignores colour and tints the shape — black on a light menu bar, white on a dark one. Light/dark is then automatic. 44 px keeps >2× pixels for the 18 pt slot on Retina. |
| `tray-color.png` (32 px, full colour) | Windows, Linux | Neither recolours tray icons; both conventionally show the app's own artwork. Checked against Windows 11's light (`#f3f3f3`) and dark (`#202020`) taskbars at 16/24/32 px — the blue reads on both, so no theme detection is needed. |

⚠ **Never swap or unify them.** The black template on a dark Windows taskbar is a
black-on-black smudge. `tray_glyph::tests::the_two_glyphs_are_not_interchangeable`
asserts the distinguishing property (template is pure black; colour is not) rather
than a filename, so the mix-up fails the build instead of shipping.

### Regenerating from `tray-source.svg`

There is no SVG rasteriser in this toolchain, so Chrome headless does it. The
source art sits inside a padded viewBox (`0 0 800.76 750.21`) while the drawing
itself is only `53.59 56.79 644.10 623.04` — rendering the raw viewBox wastes ~20%
of the bitmap on empty margin and leaves the glyph visibly small in the menu bar,
hence the tightened viewBox below.

```bash
# 1. tighten the viewBox to the artwork's real bounding box
sed 's|viewBox="0 0 800.76 750.21"|viewBox="53.59 56.79 644.10 623.04"|' \
    tray-source.svg > /tmp/tight.svg

# 2. wrap it so the glyph fills 88% of a square canvas, transparent background.
#    For tray-macos.png add `filter:brightness(0);` to the svg rule — that drives
#    every colour channel to black while leaving alpha untouched, which is exactly
#    what a template image is.
cat > /tmp/g.html <<'HTML'
<html><head><style>
  html,body{margin:0;padding:0;background:transparent;}
  body{display:flex;align-items:center;justify-content:center;}
  svg{width:88%;height:88%;}
</style></head><body>
HTML
cat /tmp/tight.svg >> /tmp/g.html && echo '</body></html>' >> /tmp/g.html

# 3. screenshot at the target size (44 for macOS, 32 for the colour one)
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --headless --disable-gpu --no-sandbox --hide-scrollbars \
  --default-background-color=00000000 \
  --window-size=44,44 --screenshot=tray-macos.png file:///tmp/g.html
```

Re-measure the bounding box if the art changes: load the SVG in a page and read
`svg.getBBox()` — the numbers above are that measurement, not a guess.

The output must stay 8-bit RGBA; `tray_glyph::decode` rejects anything else rather
than silently colour-reducing the icon.
