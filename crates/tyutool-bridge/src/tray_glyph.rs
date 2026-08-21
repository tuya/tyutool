//! The menu-bar / notification-area glyph, decoded from embedded artwork.
//!
//! Two assets, because "works in light and dark mode" is a different mechanism on
//! each platform and using the wrong one produces an **invisible** icon rather
//! than an ugly one:
//!
//! * **macOS** takes a *template* image: only the alpha channel is used, and the
//!   system tints the shape — black on a light menu bar, white on a dark one. So
//!   the asset must be a black silhouette; light/dark adaptation is then free and
//!   automatic. Handing macOS the colour artwork with `template = true` throws the
//!   colours away anyway, and handing it colour with `template = false` gives a
//!   glyph that does not follow the menu bar at all.
//! * **Windows and Linux** do not recolour tray icons — the notification area and
//!   an appindicator show the app's own artwork, which is also the platform
//!   convention. The colour logo was checked against both Windows 11 taskbar
//!   shades (`#f3f3f3` light, `#202020` dark) at 16/24/32 px and reads on both, so
//!   no theme detection is needed. The black template, by contrast, would be
//!   *invisible* on a dark taskbar — which is exactly the mix-up
//!   [`tests::the_two_glyphs_are_not_interchangeable`] exists to prevent.
//!
//! Both assets are embedded on every platform (a few KB) rather than `cfg`-picked
//! at compile time, so the invariants below are checked on every developer machine
//! and in CI regardless of which OS is running the tests.
//!
//! Regenerating them from `icons/tray-source.svg` is documented in `icons/README.md`.

/// Black silhouette + alpha, for macOS's template rendering. 44 px so an 18 pt
/// menu-bar slot still has >2× pixels on a Retina display.
const MACOS_TEMPLATE_PNG: &[u8] = include_bytes!("../icons/tray-macos.png");

/// Full-colour artwork for Windows / Linux, 32 px (the notification area asks for
/// 16–24 pt depending on DPI and scales down from this).
const COLOUR_PNG: &[u8] = include_bytes!("../icons/tray-color.png");

/// Decoded artwork, ready for `tray_icon::Icon::from_rgba`.
pub struct Glyph {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Whether the platform should be told to treat this as a template image.
    /// Travels with the pixels so the flag and the artwork cannot disagree.
    pub is_template: bool,
}

/// The glyph for the platform this binary was built for.
pub fn for_this_platform() -> anyhow::Result<Glyph> {
    if cfg!(target_os = "macos") {
        macos_template()
    } else {
        colour()
    }
}

/// Not `pub`: `for_this_platform` branches on `cfg!()`, which is a **runtime**
/// check, so both arms compile on every target and both functions are always
/// reachable — no `dead_code` warning to dodge, and no reason to widen the
/// library's surface. (An earlier revision made them `pub` on exactly that wrong
/// assumption.)
fn macos_template() -> anyhow::Result<Glyph> {
    decode(MACOS_TEMPLATE_PNG, true).map_err(|e| anyhow::anyhow!("tray template glyph: {e}"))
}

fn colour() -> anyhow::Result<Glyph> {
    decode(COLOUR_PNG, false).map_err(|e| anyhow::anyhow!("tray colour glyph: {e}"))
}

/// Straight RGBA8 decode. Anything else is a build-time asset mistake, so it is
/// reported rather than converted — a silently colour-reduced tray icon is much
/// harder to notice than a startup error.
///
/// The format check is not decorative: `png::Decoder::new` uses
/// `Transformations::IDENTITY`, so `OutputInfo` reports the file's **raw** IHDR
/// type with no auto-expansion. A palette+tRNS, 16-bit or grayscale PNG would
/// otherwise be read as RGBA8 and render as garbage.
fn decode(bytes: &[u8], is_template: bool) -> anyhow::Result<Glyph> {
    let mut reader = png::Decoder::new(bytes).read_info()?;
    let mut rgba = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut rgba)?;

    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        anyhow::bail!(
            "expected 8-bit RGBA, got {:?}/{:?} — re-export the asset",
            info.color_type,
            info.bit_depth
        );
    }
    // `rgba` is sized for the whole canvas (`output_buffer_size`) but `next_frame`
    // only fills `buffer_size()`. Those differ for an **APNG sub-frame** whose
    // `fcTL` region is smaller than the canvas — ordinary interlacing does not do
    // this, it is de-interlaced inside `next_frame`.
    //
    // Rejected rather than papered over. This replaced a `truncate` to
    // `buffer_size()`, which was the worse of the two silent options: it left a
    // buffer shorter than `width * height * 4`, so the failure surfaced later as an
    // opaque `Icon::from_rgba` error instead of here. Leaving the tail zeroed would
    // be worse still — a half-drawn icon that looks like a design choice.
    if info.buffer_size() != rgba.len() {
        anyhow::bail!(
            "frame covers {} of {} canvas bytes — animated or sub-frame PNGs are not \
             tray artwork; export a single full-canvas frame",
            info.buffer_size(),
            rgba.len()
        );
    }

    Ok(Glyph {
        rgba,
        width: info.width,
        height: info.height,
        is_template,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pixels with any opacity at all.
    fn visible_pixels(glyph: &Glyph) -> Vec<[u8; 4]> {
        glyph
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[3] > 0)
            .map(|px| [px[0], px[1], px[2], px[3]])
            .collect()
    }

    /// Both assets have to be real artwork: a decode that silently produced an
    /// empty or fully-opaque buffer would leave either a blank menu bar or a solid
    /// square, and neither is distinguishable from "the icon failed to load".
    #[test]
    fn both_glyphs_decode_to_artwork_with_a_shape_in_them() {
        for (name, glyph) in [
            ("macos template", macos_template().expect("decode")),
            ("colour", colour().expect("decode")),
        ] {
            assert_eq!(
                glyph.rgba.len() as u32,
                glyph.width * glyph.height * 4,
                "{name}: buffer must be exactly RGBA8 for its dimensions"
            );
            let visible = visible_pixels(&glyph).len();
            let total = (glyph.width * glyph.height) as usize;
            // A logo, not a blank and not a filled rectangle: the flower leaves the
            // corners and the centre star transparent.
            assert!(
                visible > total / 20,
                "{name}: only {visible}/{total} pixels are visible — the glyph is effectively blank"
            );
            assert!(
                visible < total,
                "{name}: every pixel is opaque — a solid block, not a logo"
            );
        }
    }

    /// **The mix-up that produces an invisible icon.**
    ///
    /// macOS needs a black silhouette (it tints by alpha, so colour is discarded
    /// and a dark menu bar gets a white glyph for free). Windows and Linux do *not*
    /// recolour anything, so that same black silhouette would be a black-on-black
    /// smudge on a dark taskbar.
    ///
    /// So the two assets must never be swapped or unified, and this asserts the
    /// property that distinguishes them rather than a file name: the template is
    /// entirely black, the colour artwork is not.
    #[test]
    fn the_two_glyphs_are_not_interchangeable() {
        let template = macos_template().expect("decode");
        assert!(
            template.is_template,
            "the macOS asset must be flagged as a template or the system will not tint it"
        );
        let coloured_pixels: Vec<_> = visible_pixels(&template)
            .into_iter()
            .filter(|px| px[0] != 0 || px[1] != 0 || px[2] != 0)
            .collect();
        assert!(
            coloured_pixels.is_empty(),
            "the macOS template must be pure black + alpha, found {} coloured pixels \
             (e.g. {:?}) — macOS discards colour, so this is the colour asset in the \
             wrong slot",
            coloured_pixels.len(),
            coloured_pixels.first()
        );

        let colour = colour().expect("decode");
        assert!(
            !colour.is_template,
            "the Windows/Linux asset must not be flagged as a template"
        );
        assert!(
            visible_pixels(&colour)
                .iter()
                .any(|px| px[0] != 0 || px[1] != 0 || px[2] != 0),
            "the Windows/Linux glyph is entirely black — it would be invisible on a \
             dark taskbar; this is the macOS template in the wrong slot"
        );
    }

    /// The glyph the running platform gets is the one that platform can render.
    #[test]
    fn this_platform_gets_the_glyph_its_compositor_understands() {
        let glyph = for_this_platform().expect("decode");
        assert_eq!(
            glyph.is_template,
            cfg!(target_os = "macos"),
            "template rendering is a macOS-only mechanism"
        );
    }

    /// Encode a minimal PNG so the rejection path can be exercised without
    /// committing a deliberately-broken asset to the repo.
    fn encode_png(colour: png::ColorType, depth: png::BitDepth, bytes_per_px: usize) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 2, 2);
            encoder.set_color(colour);
            encoder.set_depth(depth);
            let mut writer = encoder.write_header().expect("write header");
            writer
                .write_image_data(&vec![0u8; 4 * bytes_per_px])
                .expect("write data");
        }
        out
    }

    /// A PNG that is not 8-bit RGBA must be refused, loudly.
    ///
    /// Closes a gap the mutation pass found: both committed assets are already
    /// RGBA8, so **removing the format guard entirely left every test green**. The
    /// guard matters because `png` is configured with `Transformations::IDENTITY` —
    /// nothing expands a palette or grayscale image on the way out, so without the
    /// check those bytes would be reinterpreted as RGBA and drawn as noise.
    #[test]
    fn a_png_that_is_not_rgba8_is_refused_rather_than_misread() {
        for (label, colour, depth, bytes_per_px) in [
            (
                "grayscale 8-bit",
                png::ColorType::Grayscale,
                png::BitDepth::Eight,
                1,
            ),
            (
                "rgb 8-bit (no alpha)",
                png::ColorType::Rgb,
                png::BitDepth::Eight,
                3,
            ),
            (
                "rgba 16-bit",
                png::ColorType::Rgba,
                png::BitDepth::Sixteen,
                8,
            ),
        ] {
            let png_bytes = encode_png(colour, depth, bytes_per_px);
            let message = match decode(&png_bytes, false) {
                Ok(_) => panic!("{label} must not be accepted as tray artwork"),
                Err(e) => format!("{e:#}"),
            };
            assert!(
                message.contains("8-bit RGBA"),
                "{label}: the error must name what was expected, got {message:?}"
            );
        }
    }

    /// The computed flag must actually reach the platform call.
    ///
    /// Closes the other gap the mutation pass found: replacing
    /// `.with_icon_as_template(is_template)` with a hardcoded `true` in `main.rs`
    /// left **all 131 tests green**, because the unit tests only ever inspect
    /// `Glyph.is_template` in isolation. A correct value that nothing consumes is
    /// this project's most repeated defect shape, and here its consequence is a
    /// black-on-black icon on a dark Windows taskbar.
    ///
    /// Source-order, like the guards in `proc`: `TrayShell::build` needs a real
    /// status item, so there is no seam to assert through on a CI runner.
    #[test]
    fn the_template_flag_is_threaded_through_to_the_platform_not_hardcoded() {
        let main_src = include_str!("main.rs");
        let call = main_src
            .lines()
            .find(|line| line.contains(".with_icon_as_template("))
            .expect("main.rs must configure template rendering");

        assert!(
            call.contains(".with_icon_as_template(is_template)"),
            "the flag must come from the artwork, not be spelled out: {}",
            call.trim()
        );
    }
}
