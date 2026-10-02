//! Visual classification + colours. Everything colour-related lives here.
//!
//! The reference does NOT colour by extension (the same .swift folder shows four colours, and a
//! folder flips from orange to blue depending on the column it is in). Colours are a recency
//! ramp: white = just modified → grey → saturated = old. Columns on the selection path use the
//! orange hue; preview columns use the blue hue. Kind-based colours are kept as an alternate mode.

use crate::index::classify;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn hex(v: u32) -> Rgb {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    pub fn to_u32(self) -> u32 {
        (self.0 as u32) << 16 | (self.1 as u32) << 8 | self.2 as u32
    }
}

pub const BACKGROUND: Rgb = Rgb::hex(0x000000);
pub const SELECTED: Rgb = Rgb::hex(0xe60000);
pub const CONNECTOR: Rgb = Rgb::hex(0x8ab0ff);
pub const PATH_CONNECTOR: Rgb = Rgb::hex(0xf06c04);
pub const TEXT: Rgb = Rgb::hex(0xfcfcfc);
pub const DIM: Rgb = Rgb::hex(0x82808a);
pub const RULE: Rgb = Rgb::hex(0x1c1b20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hue {
    Path,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Age,
    Kind,
}

impl ColorMode {
    pub fn parse(s: &str) -> ColorMode {
        if s == "kind" { ColorMode::Kind } else { ColorMode::Age }
    }
}

/// Age ramp stops (t, colour), sampled from the reference frames (brightest glyph pixels).
/// The blue ramp bends through violet: green fades faster than red on the way to #0000ec.
const PATH_RAMP: &[(f64, Rgb)] = &[(0.0, Rgb::hex(0xfcfcfc)), (0.5, Rgb::hex(0x8a8886)), (1.0, Rgb::hex(0xf06c04))];
const PREVIEW_RAMP: &[(f64, Rgb)] = &[
    (0.0, Rgb::hex(0xfcfcfc)),
    (0.5, Rgb::hex(0x82808a)),
    (0.66, Rgb::hex(0x5c50a8)),
    (0.86, Rgb::hex(0x2410d4)),
    (1.0, Rgb::hex(0x0000ec)),
];

pub fn kind_color(kind: &str) -> Rgb {
    match kind {
        "directory" => Rgb::hex(0xfcfcfc),
        "hidden" => Rgb::hex(0x82808a),
        "code" => Rgb::hex(0x8ab0ff),
        "text" => Rgb::hex(0xb8b8b8),
        "image" => Rgb::hex(0x6058a4),
        "document" => Rgb::hex(0xf06c04),
        "special" => Rgb::hex(0xe07020),
        _ => Rgb::hex(0x0000ec),
    }
}

/// Age curve: log-scale between `AGE_MIN_MS` (t = 0) and `max_ms` (t = 1).
pub const AGE_MIN_MS: f64 = 10.0 * 60.0 * 1000.0;
pub const DAY_MS: f64 = 24.0 * 3600.0 * 1000.0;
pub const DEFAULT_AGE_MAX_MS: f64 = 730.0 * DAY_MS;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub mode: ColorMode,
    pub age_max_ms: f64,
}

impl Default for Colors {
    fn default() -> Self {
        Colors { mode: ColorMode::Age, age_max_ms: DEFAULT_AGE_MAX_MS }
    }
}

impl Colors {
    pub fn new(mode: ColorMode, age_max_ms: f64) -> Colors {
        Colors { mode, age_max_ms: age_max_ms.max(AGE_MIN_MS * 2.0) }
    }

    fn log_span(&self) -> f64 {
        (self.age_max_ms / AGE_MIN_MS).ln()
    }

    pub fn age_t(&self, mtime_ms: f64, now_ms: f64) -> f64 {
        let age = now_ms - mtime_ms;
        // Also true for NaN (unknown mtime): treated as just modified, like the original.
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(age > AGE_MIN_MS) {
            return 0.0;
        }
        ((age / AGE_MIN_MS).ln() / self.log_span()).min(1.0)
    }

    pub fn age_for_t(&self, t: f64) -> f64 {
        AGE_MIN_MS * (t * self.log_span()).exp()
    }

    pub fn color_for(&self, name: &str, is_dir: bool, is_hidden: bool, t: f64, hue: Hue) -> Rgb {
        match self.mode {
            ColorMode::Age => ramp_color(t, hue),
            ColorMode::Kind => kind_color(classify(name, is_dir, is_hidden)),
        }
    }
}

pub fn ramp_color(t: f64, hue: Hue) -> Rgb {
    let stops = match hue {
        Hue::Path => PATH_RAMP,
        Hue::Preview => PREVIEW_RAMP,
    };
    let tt = t.clamp(0.0, 1.0);
    let mut i = 1;
    while i < stops.len() - 1 && tt > stops[i].0 {
        i += 1;
    }
    let (t0, from) = stops[i - 1];
    let (t1, to) = stops[i];
    let u = if t1 > t0 { (tt - t0) / (t1 - t0) } else { 0.0 };
    let ch = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * u).round() as u8;
    Rgb(ch(from.0, to.0), ch(from.1, to.1), ch(from.2, to.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_endpoints_and_midpoint() {
        assert_eq!(ramp_color(0.0, Hue::Path), Rgb::hex(0xfcfcfc));
        assert_eq!(ramp_color(1.0, Hue::Path), Rgb::hex(0xf06c04));
        assert_eq!(ramp_color(0.5, Hue::Preview), Rgb::hex(0x82808a));
        assert_eq!(ramp_color(2.0, Hue::Preview), Rgb::hex(0x0000ec));
    }

    #[test]
    fn age_curve_round_trips() {
        let c = Colors::default();
        for t in [0.1, 0.37, 0.8, 1.0] {
            let age = c.age_for_t(t);
            assert!((c.age_t(0.0, age) - t).abs() < 1e-9);
        }
        assert_eq!(c.age_t(1000.0, 1000.0), 0.0);
    }
}
