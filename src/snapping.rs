//! Object snapping and smart-guide computation for interactive dragging.
//!
//! Deliberately pure and framework-agnostic, mirroring `model.rs`: this
//! module only reads the document and returns numbers/lines, so it never
//! touches SVG rendering or `EditorState`/history. The canvas asks it "given
//! this candidate bounding box, how far should it nudge to align, and what
//! guide lines justify that" on every pointer move, then throws the answer
//! away once the drag ends — guides are never persisted anywhere.

use std::collections::BTreeSet;

use crate::model::{DesignDocument, Geometry, ObjectId};

/// How close (in canvas units) a moving edge/center must be to a target
/// before it snaps to it.
pub const SNAP_THRESHOLD: f64 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideOrientation {
    Vertical,
    Horizontal,
}

/// One temporary guide line: `position` is the coordinate along its own
/// axis (x for a vertical line, y for a horizontal one), `start`/`end` span
/// the cross axis so the line is drawn only across the relevant objects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub orientation: GuideOrientation,
    pub position: f64,
    pub start: f64,
    pub end: f64,
}

/// A candidate alignment position on one axis, with the cross-axis extent
/// it should draw a guide across if it's the one that ends up matching.
struct AxisTarget {
    position: f64,
    extent: (f64, f64),
}

fn collect_x_targets(document: &DesignDocument, exclude: &BTreeSet<ObjectId>, canvas_width: f64, canvas_height: f64) -> Vec<AxisTarget> {
    let mut targets = vec![
        AxisTarget { position: 0.0, extent: (0.0, canvas_height) },
        AxisTarget { position: canvas_width / 2.0, extent: (0.0, canvas_height) },
        AxisTarget { position: canvas_width, extent: (0.0, canvas_height) },
    ];
    for object in &document.objects {
        if object.kind.is_group() || exclude.contains(&object.id) {
            continue;
        }
        let g = &object.geometry;
        let extent = (g.y, g.y + g.height);
        targets.push(AxisTarget { position: g.x, extent });
        targets.push(AxisTarget { position: g.x + g.width / 2.0, extent });
        targets.push(AxisTarget { position: g.x + g.width, extent });
    }
    targets
}

fn collect_y_targets(document: &DesignDocument, exclude: &BTreeSet<ObjectId>, canvas_width: f64, canvas_height: f64) -> Vec<AxisTarget> {
    let mut targets = vec![
        AxisTarget { position: 0.0, extent: (0.0, canvas_width) },
        AxisTarget { position: canvas_height / 2.0, extent: (0.0, canvas_width) },
        AxisTarget { position: canvas_height, extent: (0.0, canvas_width) },
    ];
    for object in &document.objects {
        if object.kind.is_group() || exclude.contains(&object.id) {
            continue;
        }
        let g = &object.geometry;
        let extent = (g.x, g.x + g.width);
        targets.push(AxisTarget { position: g.y, extent });
        targets.push(AxisTarget { position: g.y + g.height / 2.0, extent });
        targets.push(AxisTarget { position: g.y + g.height, extent });
    }
    targets
}

struct AxisSnap {
    delta: f64,
    guide_position: f64,
    target_extent: (f64, f64),
}

/// Tests the moving span's start/center/end edges against every target on
/// this axis and keeps the closest match within `threshold`, if any.
fn snap_axis(moving_start: f64, moving_size: f64, targets: &[AxisTarget], threshold: f64) -> Option<AxisSnap> {
    let edges = [moving_start, moving_start + moving_size / 2.0, moving_start + moving_size];
    let mut best: Option<(f64, AxisSnap)> = None;
    for &edge in &edges {
        for target in targets {
            let diff = target.position - edge;
            let distance = diff.abs();
            if distance <= threshold && best.as_ref().is_none_or(|(best_distance, _)| distance < *best_distance) {
                best = Some((
                    distance,
                    AxisSnap { delta: diff, guide_position: target.position, target_extent: target.extent },
                ));
            }
        }
    }
    best.map(|(_, snap)| snap)
}

/// Given the bounding box a selection would occupy at its current drag
/// position, returns the (dx, dy) nudge that snaps it to the nearest
/// alignment target on each axis (0.0 if nothing is within threshold) plus
/// the guide lines to display for whatever it snapped to. `exclude` should
/// contain every id being dragged, so an object never snaps to itself.
pub fn compute_snap(
    document: &DesignDocument,
    exclude: &BTreeSet<ObjectId>,
    bbox: Geometry,
    canvas_width: f64,
    canvas_height: f64,
) -> (f64, f64, Vec<Guide>) {
    let x_targets = collect_x_targets(document, exclude, canvas_width, canvas_height);
    let y_targets = collect_y_targets(document, exclude, canvas_width, canvas_height);

    let mut guides = Vec::new();
    let mut dx = 0.0;
    let mut dy = 0.0;

    if let Some(snap) = snap_axis(bbox.x, bbox.width, &x_targets, SNAP_THRESHOLD) {
        dx = snap.delta;
        let start = snap.target_extent.0.min(bbox.y);
        let end = snap.target_extent.1.max(bbox.y + bbox.height);
        guides.push(Guide { orientation: GuideOrientation::Vertical, position: snap.guide_position, start, end });
    }

    if let Some(snap) = snap_axis(bbox.y, bbox.height, &y_targets, SNAP_THRESHOLD) {
        dy = snap.delta;
        let start = snap.target_extent.0.min(bbox.x);
        let end = snap.target_extent.1.max(bbox.x + bbox.width);
        guides.push(Guide { orientation: GuideOrientation::Horizontal, position: snap.guide_position, start, end });
    }

    (dx, dy, guides)
}
