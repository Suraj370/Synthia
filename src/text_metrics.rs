//! Heuristic text measurement used to auto-size and word-wrap text objects.
//!
//! This deliberately approximates glyph widths from font size rather than
//! measuring real rendered metrics: the rest of this codebase only ever
//! touches the DOM through Yew's own event/cast helpers (see
//! `TargetCast::target_unchecked_into` elsewhere), never raw `wasm-bindgen`
//! JS casts, and real text measurement (a `<canvas>` 2D context) would be
//! the first thing here that needs that. The tradeoff is a box that's close
//! but not pixel-perfect to the actual rendered glyphs — acceptable for
//! auto-size/wrap, same spirit as the existing "rotation not compensated
//! for" simplification in `canvas_area`'s resize math.
//!
//! Pure and framework-agnostic like `model.rs`/`snapping.rs`: no SVG, no
//! Yew, just numbers in and numbers out.

use crate::model::{FontFamily, TextProperties};

const MIN_WIDTH: f64 = 24.0;
const MIN_HEIGHT: f64 = 20.0;

/// Padding applied around text content, both in the static SVG rendering
/// and in the live editing textarea overlay, so the two line up exactly.
pub const HORIZONTAL_PADDING: f64 = 8.0;
pub const VERTICAL_PADDING: f64 = 4.0;

/// Rough average glyph width as a fraction of font size. Sans faces run
/// narrower than serif; monospace is uniform by definition.
fn avg_glyph_width_factor(family: FontFamily) -> f64 {
    match family {
        FontFamily::CourierNew => 0.6,
        FontFamily::Georgia | FontFamily::TimesNewRoman | FontFamily::SystemSerif => 0.52,
        FontFamily::Inter | FontFamily::Arial | FontFamily::Helvetica | FontFamily::SystemSansSerif => 0.56,
    }
}

/// The (width, height) an `Auto`-size text object should occupy for its
/// current content and font settings.
pub fn auto_size(props: &TextProperties) -> (f64, f64) {
    let factor = avg_glyph_width_factor(props.font_family);
    let lines: Vec<&str> = props.content.split('\n').collect();
    let line_count = lines.len().max(1);
    let max_chars = lines.iter().map(|line| line.chars().count()).max().unwrap_or(0);

    let glyph_width = props.font_size * factor;
    let letter_spacing_total = props.letter_spacing * max_chars.saturating_sub(1) as f64;
    let width = (max_chars as f64 * glyph_width + letter_spacing_total + HORIZONTAL_PADDING * 2.0).max(MIN_WIDTH);
    let height = (line_count as f64 * props.font_size * props.line_height + VERTICAL_PADDING * 2.0).max(MIN_HEIGHT);

    (width, height)
}

/// Word-wraps `props.content` to fit within `max_width`, honoring existing
/// newlines as hard paragraph breaks. Used for `Fixed`-size text boxes,
/// where the box width is user-controlled instead of following content.
pub fn wrap_lines(props: &TextProperties, max_width: f64) -> Vec<String> {
    let factor = avg_glyph_width_factor(props.font_family);
    let glyph_width = (props.font_size * factor + props.letter_spacing).max(1.0);
    let usable_width = (max_width - HORIZONTAL_PADDING * 2.0).max(glyph_width);
    let max_chars = (usable_width / glyph_width).floor().max(1.0) as usize;

    let mut result = Vec::new();
    for paragraph in props.content.split('\n') {
        if paragraph.is_empty() {
            result.push(String::new());
            continue;
        }

        let mut current = String::new();
        for word in paragraph.split(' ') {
            let mut word = word;
            // A lone word longer than the box: hard-break it so it can't
            // overflow forever.
            while word.chars().count() > max_chars {
                let split_at = word.char_indices().nth(max_chars).map(|(i, _)| i).unwrap_or(word.len());
                let (head, tail) = word.split_at(split_at);
                if !current.is_empty() {
                    result.push(std::mem::take(&mut current));
                }
                result.push(head.to_string());
                word = tail;
            }

            let candidate_len = if current.is_empty() { word.chars().count() } else { current.chars().count() + 1 + word.chars().count() };
            if candidate_len > max_chars && !current.is_empty() {
                result.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        result.push(current);
    }
    result
}
