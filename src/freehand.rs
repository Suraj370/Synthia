//! Pure helpers for the Pencil tool: point simplification and smooth SVG
//! path generation. Framework-agnostic like `model.rs`/`snapping.rs` — no
//! Yew, no DOM.
//!
//! Two separate concerns keep the point count (and therefore the stored
//! `PathProperties`/exported SVG) small without a drawing library:
//! - `MIN_POINT_DISTANCE` (used while recording, in `canvas_area.rs`) skips
//!   pointer-move samples that haven't moved far enough to matter, so a
//!   slow drag doesn't pile up thousands of near-duplicate points.
//! - `simplify` (Ramer-Douglas-Peucker) runs once on pointer-up and drops
//!   points that don't meaningfully change the curve's shape.
//!
//! The recorded points are still just a polyline; `smooth_path_d` turns
//! them into a rounded SVG path (quadratic Béziers through the midpoint of
//! each consecutive pair) purely at render time — the document only ever
//! stores the plain points (see `model.rs`'s `PathProperties`), never SVG
//! markup.

/// Minimum distance (canvas units) between consecutive recorded points
/// while actively dragging the Pencil tool.
pub const MIN_POINT_DISTANCE: f64 = 3.0;

/// Ramer-Douglas-Peucker epsilon (canvas units): a point is dropped if it
/// lies within this distance of the line between its neighbors.
pub const SIMPLIFY_EPSILON: f64 = 1.5;

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Perpendicular distance from `point` to the infinite line through
/// `start`/`end` (or to `start` itself if they coincide).
fn perpendicular_distance(point: (f64, f64), start: (f64, f64), end: (f64, f64)) -> f64 {
    let line_len = distance(start, end);
    if line_len < 1e-9 {
        return distance(point, start);
    }
    let numerator = ((end.1 - start.1) * point.0 - (end.0 - start.0) * point.1 + end.0 * start.1 - end.1 * start.0).abs();
    numerator / line_len
}

/// Ramer-Douglas-Peucker simplification: keeps `points[0]` and
/// `points[last]`, recursively keeping only points that deviate from the
/// straight line between their neighbors by more than `epsilon`. Runs in
/// O(n log n) typical case, well within budget for a single pointer-up.
pub fn simplify(points: &[(f64, f64)], epsilon: f64) -> Vec<(f64, f64)> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;

    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((start_idx, end_idx)) = stack.pop() {
        if end_idx <= start_idx + 1 {
            continue;
        }
        let (start, end) = (points[start_idx], points[end_idx]);
        let mut farthest_idx = start_idx;
        let mut farthest_dist = 0.0;
        for i in (start_idx + 1)..end_idx {
            let dist = perpendicular_distance(points[i], start, end);
            if dist > farthest_dist {
                farthest_dist = dist;
                farthest_idx = i;
            }
        }
        if farthest_dist > epsilon {
            keep[farthest_idx] = true;
            stack.push((start_idx, farthest_idx));
            stack.push((farthest_idx, end_idx));
        }
    }

    points.iter().zip(keep).filter_map(|(&point, kept)| kept.then_some(point)).collect()
}

/// Builds a smooth SVG path `d` string through `points` (already in
/// whatever coordinate space the caller wants — local object space for the
/// canvas renderer, canvas space for export), using quadratic Béziers
/// through each pair's midpoint. A single point renders as a zero-length
/// segment (a dot, via `stroke-linecap: round`); an empty slice yields "".
pub fn smooth_path_d(points: &[(f64, f64)]) -> String {
    match points.len() {
        0 => String::new(),
        1 => format!("M {} {} L {} {}", points[0].0, points[0].1, points[0].0, points[0].1),
        _ => {
            let mut d = format!("M {} {}", points[0].0, points[0].1);
            for i in 1..points.len() - 1 {
                let mid_x = (points[i].0 + points[i + 1].0) / 2.0;
                let mid_y = (points[i].1 + points[i + 1].1) / 2.0;
                d.push_str(&format!(" Q {} {} {} {}", points[i].0, points[i].1, mid_x, mid_y));
            }
            let last = points[points.len() - 1];
            d.push_str(&format!(" L {} {}", last.0, last.1));
            d
        }
    }
}
