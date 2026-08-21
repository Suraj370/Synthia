//! Color representation and pure conversions, shared by the document model
//! (`model::Color` lives here) and the color-picker UI. Framework-agnostic
//! like `model.rs` — nothing here knows about Yew or SVG.
//!
//! `Color` (RGB + separate alpha) is the one representation stored on the
//! document and used everywhere a color renders. `Hsv` exists only to
//! drive the picker's saturation/value area and hue slider — it's derived
//! from a `Color` when a picker session starts and converted straight back,
//! never stored anywhere persistent (see `model.rs` module docs on keeping
//! UI-only state out of the document).

use serde::{Deserialize, Serialize};

/// An RGB color with a separate 0.0-1.0 alpha channel. The one color
/// representation used throughout Apollo: hex input, RGB fields, the
/// picker's gradients, and an eyedropper sample all normalize into this
/// same shape, so nothing downstream has to guess where a color came from.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f64,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub fn with_alpha(self, a: f64) -> Self {
        Self { a: a.clamp(0.0, 1.0), ..self }
    }

    /// `#RRGGBB`, uppercase, alpha not included — alpha always has its own
    /// field/control alongside the hex input, never folded into it.
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// CSS `rgba()` — the one string form every renderer/style uses, so
    /// alpha is never silently dropped by an ad hoc `#RRGGBB`-only format.
    pub fn to_css(self) -> String {
        format!("rgba({}, {}, {}, {})", self.r, self.g, self.b, self.a)
    }

    pub fn to_hsv(self) -> Hsv {
        rgb_to_hsv(self.r, self.g, self.b)
    }
}

/// Parses `#RGB` or `#RRGGBB` (leading `#` optional). Returns `None` for
/// anything else rather than guessing, so a bad paste into the hex field
/// just leaves the color unchanged instead of committing garbage.
pub fn parse_hex(input: &str) -> Option<(u8, u8, u8)> {
    let hex = input.trim().trim_start_matches('#');
    match hex.len() {
        6 => Some((u8::from_str_radix(&hex[0..2], 16).ok()?, u8::from_str_radix(&hex[2..4], 16).ok()?, u8::from_str_radix(&hex[4..6], 16).ok()?)),
        3 => {
            let mut chars = hex.chars();
            let expand = |c: char| u8::from_str_radix(&c.to_string().repeat(2), 16).ok();
            Some((expand(chars.next()?)?, expand(chars.next()?)?, expand(chars.next()?)?))
        }
        _ => None,
    }
}

/// Hue in degrees (0-360), saturation and value each 0.0-1.0 — the picker's
/// working representation while a gesture is in progress, since dragging in
/// the saturation/value square or the hue slider is far more natural in
/// this space than in raw RGB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub h: f64,
    pub s: f64,
    pub v: f64,
}

pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> Hsv {
    let r = r as f64 / 255.0;
    let g = g as f64 / 255.0;
    let b = b as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let h = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta).rem_euclid(6.0))
    } else if max == g {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };

    let s = if max == 0.0 { 0.0 } else { delta / max };
    Hsv { h, s, v: max }
}

pub fn hsv_to_rgb(hsv: Hsv) -> (u8, u8, u8) {
    let Hsv { h, s, v } = hsv;
    let c = v * s;
    let h_prime = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (h_prime.rem_euclid(2.0) - 1.0).abs());
    let (r1, g1, b1) = match h_prime as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (to_byte(r1 + m), to_byte(g1 + m), to_byte(b1 + m))
}

fn to_byte(channel: f64) -> u8 {
    (channel * 255.0).round().clamp(0.0, 255.0) as u8
}
