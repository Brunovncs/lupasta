//! Fonts and text painting. The whole layout is measured in 10 px cells, so a different monospace
//! font is not allowed to change the cell: its size is set so its advance is exactly one cell, its
//! caps are centred where Iosevka's are, and "…" is squeezed into one cell.

use gpui::{App, Font, FontFeatures, FontWeight, Hsla, SharedString, TextAlign, TextRun, Window, font, point, px};
use lupasta::layout::{ELLIPSIS, cell_width};
use lupasta::prefs;
use std::borrow::Cow;
use std::sync::Arc;

pub const BUNDLED: &[&[u8]] = &[
    include_bytes!("../../assets/fonts/Iosevka-Medium.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Medium.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexMono-Medium.ttf"),
];

/// Bundled faces are Medium cuts: the regular weight reads too thin on Windows next to the
/// macOS-rendered reference. Their registered family name differs between platforms (with or
/// without the weight), so each id lists both spellings.
fn candidates(id: &str) -> Vec<String> {
    let family = prefs::font_family(id);
    let base = family.trim_end_matches(" Medium");
    if base == family { vec![family.to_string()] } else { vec![family.to_string(), base.to_string()] }
}

pub fn load_bundled(cx: &App) {
    let fonts = BUNDLED.iter().map(|b| Cow::Borrowed(*b)).collect();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("could not load bundled fonts: {e}");
    }
}

/// Iosevka at 20 px with text_y -1.5 is the measured reference; everything else is fitted to it.
const REFERENCE_SIZE: f32 = 20.0;
const REFERENCE_TEXT_Y: f32 = -1.5;

#[derive(Debug, Clone)]
pub struct FontFit {
    pub font: Font,
    pub font_size: f32,
    pub text_y: f32,
    /// The font's "…" is wider than a cell (Iosevka's is two): draw it as three squeezed dots.
    pub squeeze_ellipsis: bool,
}

struct Measured {
    advance: f32,
    ellipsis: f32,
    ascent: f32,
    descent: f32,
    cap: f32,
}

fn make_font(family: &str) -> Font {
    Font { features: FontFeatures(Arc::new(vec![("calt".into(), 0), ("liga".into(), 0), ("kern".into(), 0)])), weight: FontWeight::NORMAL, ..font(family.to_string()) }
}

fn measure(cx: &App, f: &Font) -> Measured {
    let ts = cx.text_system();
    let id = ts.resolve_font(f);
    let size = px(100.0);
    let adv = |c: char| ts.advance(id, size, c).map(|s| f32::from(s.width) / 100.0).unwrap_or(0.5);
    Measured {
        advance: adv('M'),
        ellipsis: adv(ELLIPSIS),
        ascent: f32::from(ts.ascent(id, size)) / 100.0,
        descent: f32::from(ts.descent(id, size)) / 100.0,
        cap: f32::from(ts.cap_height(id, size)) / 100.0,
    }
}

/// Picks the font for a settings id and fits it to the cell.
pub fn fit(cx: &App, id: &str, char_w: f32, row_h: f32) -> FontFit {
    let names = cx.text_system().all_font_names();
    let family = candidates(id).into_iter().find(|c| names.iter().any(|n| n == c)).unwrap_or_else(|| prefs::font_family(id).to_string());
    let reference = candidates("iosevka").into_iter().find(|c| names.iter().any(|n| n == c)).unwrap_or_else(|| "Iosevka".into());
    let f = make_font(&family);
    let m = measure(cx, &f);
    let r = measure(cx, &make_font(&reference));
    let font_size = if m.advance > 0.0 { char_w / m.advance } else { REFERENCE_SIZE };
    let cap_centre = |x: &Measured, size: f32| (row_h - (x.ascent + x.descent) * size) / 2.0 + x.ascent * size - x.cap * size / 2.0;
    let text_y = REFERENCE_TEXT_Y + cap_centre(&r, REFERENCE_SIZE) - cap_centre(&m, font_size);
    if std::env::var_os("LUPASTA_DEBUG_FONTS").is_some() {
        let ours: Vec<&String> = names.iter().filter(|n| n.contains("Iosevka") || n.contains("JetBrains") || n.contains("Plex")).collect();
        eprintln!("fonts: {ours:?}; using {family:?}: advance {} ascent {} descent {} cap {} ellipsis {} -> size {font_size} text_y {text_y}", m.advance, m.ascent, m.descent, m.cap, m.ellipsis);
    }
    FontFit { font: f, font_size, text_y, squeeze_ellipsis: m.ellipsis * font_size > char_w * 1.05 }
}

/// Paints one line of monospace text with its row top at (x, y). Every "…" occupies one cell;
/// with `squeeze` it is drawn as three dots inside that cell.
#[allow(clippy::too_many_arguments)]
pub fn paint_line(window: &mut Window, cx: &mut App, fit: &FontFit, x: f32, y: f32, z: f32, cell: f32, row_h: f32, text: &str, color: Hsla) {
    let size = px(fit.font_size * z);
    let line_h = px(row_h * z);
    let top = y + fit.text_y * z;
    let paint = |s: &str, at: f32, window: &mut Window, cx: &mut App| {
        if s.is_empty() {
            return;
        }
        let run = TextRun { len: s.len(), font: fit.font.clone(), color, background_color: None, underline: None, strikethrough: None };
        let shaped = window.text_system().shape_line(SharedString::from(s.to_string()), size, &[run], None);
        let _ = shaped.paint(point(px(at), px(top)), line_h, TextAlign::Left, None, window, cx);
    };
    if !fit.squeeze_ellipsis || !text.contains(ELLIPSIS) {
        paint(text, x, window, cx);
        return;
    }
    let mut at = x;
    for (i, part) in text.split(ELLIPSIS).enumerate() {
        if i > 0 {
            // Three periods, each centred in a third of the cell.
            for k in 0..3 {
                paint(".", at + cell * z * (k as f32 - 1.0) / 3.0, window, cx);
            }
            at += cell * z;
        }
        paint(part, at, window, cx);
        at += cell_width(part) as f32 * cell * z;
    }
}
