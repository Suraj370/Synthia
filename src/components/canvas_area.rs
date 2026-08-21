use std::collections::BTreeSet;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{DragEvent, Element, FocusEvent, HtmlTextAreaElement, InputEvent, MouseEvent, WheelEvent};
use yew::prelude::*;

use crate::editor_state::{use_editor, EditorAction, EditorContext, Tool, MAX_ZOOM, MIN_ZOOM};
use crate::freehand;
use crate::image_import::{files_from_drop, import_image_file, ImportTarget};
use crate::model::{
    DesignDocument, DesignObject, FontStyle, FontWeight, Geometry, ImageFit, ImageProperties, LineProperties, ObjectId, ObjectKind,
    PathPoint, PathProperties, ShapeProperties, TextAlign, TextProperties, TextSizeMode, DUPLICATE_OFFSET, MIN_OBJECT_SIZE,
};
use crate::snapping::{compute_snap, Guide, GuideOrientation};
use crate::text_metrics;

const MIN_DRAG_SIZE: f64 = 2.0;
const HANDLE_SIZE: f64 = 6.0;
/// Breathing room left around the document on every side when Fit mode
/// computes its zoom, so the artboard never touches the workspace edges.
const FIT_MARGIN: f64 = 40.0;
/// How much a freshly computed Fit-mode zoom must differ from the current
/// `editor.zoom` before it's worth a `SyncFitZoom` dispatch — guards
/// against re-render/dispatch churn from floating-point noise.
const FIT_ZOOM_EPSILON: f64 = 0.001;

#[derive(Clone, Copy, PartialEq)]
struct DragState {
    start_x: f64,
    start_y: f64,
    current_x: f64,
    current_y: f64,
}

impl DragState {
    fn geometry(&self) -> Geometry {
        let x = self.start_x.min(self.current_x);
        let y = self.start_y.min(self.current_y);
        let width = (self.current_x - self.start_x).abs();
        let height = (self.current_y - self.start_y).abs();
        Geometry::new(x, y, width, height)
    }
}

/// Which edge of one axis a resize handle controls: the low edge (Start,
/// e.g. left/top), the high edge (End, e.g. right/bottom), or neither
/// (Fixed, meaning this handle doesn't move that axis at all).
#[derive(Clone, Copy, PartialEq)]
enum Edge {
    Start,
    End,
    Fixed,
}

#[derive(Clone, Copy, PartialEq)]
enum HandleKind {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl HandleKind {
    const ALL: [HandleKind; 8] = [
        HandleKind::TopLeft,
        HandleKind::Top,
        HandleKind::TopRight,
        HandleKind::Left,
        HandleKind::Right,
        HandleKind::BottomLeft,
        HandleKind::Bottom,
        HandleKind::BottomRight,
    ];

    fn h_edge(self) -> Edge {
        match self {
            HandleKind::TopLeft | HandleKind::Left | HandleKind::BottomLeft => Edge::Start,
            HandleKind::TopRight | HandleKind::Right | HandleKind::BottomRight => Edge::End,
            HandleKind::Top | HandleKind::Bottom => Edge::Fixed,
        }
    }

    fn v_edge(self) -> Edge {
        match self {
            HandleKind::TopLeft | HandleKind::Top | HandleKind::TopRight => Edge::Start,
            HandleKind::BottomLeft | HandleKind::Bottom | HandleKind::BottomRight => Edge::End,
            HandleKind::Left | HandleKind::Right => Edge::Fixed,
        }
    }

    /// This handle's center point in the object's local, untransformed
    /// coordinate space (origin at the object's top-left corner).
    fn local_position(self, width: f64, height: f64) -> (f64, f64) {
        let x = match self.h_edge() {
            Edge::Start => 0.0,
            Edge::End => width,
            Edge::Fixed => width / 2.0,
        };
        let y = match self.v_edge() {
            Edge::Start => 0.0,
            Edge::End => height,
            Edge::Fixed => height / 2.0,
        };
        (x, y)
    }

    fn cursor(self) -> &'static str {
        match self {
            HandleKind::TopLeft | HandleKind::BottomRight => "nwse-resize",
            HandleKind::TopRight | HandleKind::BottomLeft => "nesw-resize",
            HandleKind::Top | HandleKind::Bottom => "ns-resize",
            HandleKind::Left | HandleKind::Right => "ew-resize",
        }
    }
}

/// Resizes one axis given the edge being dragged, the fixed anchor
/// coordinate (the opposite, non-moving edge), and the current mouse
/// position on that axis. Returns the new (origin, size) for the axis.
fn resize_axis(anchor: f64, mouse: f64, edge: Edge, min_size: f64) -> (f64, f64) {
    let mouse = match edge {
        Edge::Start => mouse.min(anchor - min_size),
        Edge::End => mouse.max(anchor + min_size),
        Edge::Fixed => mouse,
    };
    (anchor.min(mouse), (anchor - mouse).abs())
}

/// Computes the object's new geometry while dragging `handle`, given the
/// geometry it had when the drag started and the current mouse position in
/// canvas coordinates. Rotation is intentionally not compensated for here —
/// there is no way to interactively rotate an object yet, so the common
/// case (rotation 0) is what this needs to get right.
fn compute_resized_geometry(origin: Geometry, handle: HandleKind, mouse_x: f64, mouse_y: f64) -> Geometry {
    let mut geometry = origin;

    match handle.h_edge() {
        Edge::Fixed => {}
        edge => {
            let anchor_x = match edge {
                Edge::Start => origin.x + origin.width,
                _ => origin.x,
            };
            let (x, width) = resize_axis(anchor_x, mouse_x, edge, MIN_OBJECT_SIZE);
            geometry.x = x;
            geometry.width = width;
        }
    }

    match handle.v_edge() {
        Edge::Fixed => {}
        edge => {
            let anchor_y = match edge {
                Edge::Start => origin.y + origin.height,
                _ => origin.y,
            };
            let (y, height) = resize_axis(anchor_y, mouse_y, edge, MIN_OBJECT_SIZE);
            geometry.y = y;
            geometry.height = height;
        }
    }

    geometry
}

/// When Shift is held while dragging a corner handle, rescales the resize
/// uniformly (preserving `origin`'s aspect ratio) instead of the free-form
/// width/height from `resized`, while keeping the same corner/edge anchored
/// that `resized` already anchored. No-ops for edge handles (only one axis
/// moves, so "aspect ratio" isn't meaningful there) and for zero-sized
/// origins.
fn apply_aspect_lock(origin: Geometry, handle: HandleKind, resized: Geometry) -> Geometry {
    if handle.h_edge() == Edge::Fixed || handle.v_edge() == Edge::Fixed {
        return resized;
    }
    if origin.width <= 0.0 || origin.height <= 0.0 {
        return resized;
    }

    let scale = (resized.width / origin.width)
        .max(resized.height / origin.height)
        .max(MIN_OBJECT_SIZE / origin.width)
        .max(MIN_OBJECT_SIZE / origin.height);

    let width = origin.width * scale;
    let height = origin.height * scale;

    let x = match handle.h_edge() {
        Edge::Start => origin.x + origin.width - width,
        _ => origin.x,
    };
    let y = match handle.v_edge() {
        Edge::Start => origin.y + origin.height - height,
        _ => origin.y,
    };

    Geometry { x, y, width, height, ..origin }
}

/// Step (in degrees) the Line tool's endpoint snaps to while Shift is held.
const LINE_ANGLE_STEP_DEGREES: f64 = 45.0;

/// Snaps `point` to the nearest `LINE_ANGLE_STEP_DEGREES` increment around
/// `start`, preserving the distance between them — Shift-constrained line
/// drawing (0°/45°/90°/...).
fn constrain_line_angle(start: (f64, f64), point: (f64, f64)) -> (f64, f64) {
    let dx = point.0 - start.0;
    let dy = point.1 - start.1;
    let distance = (dx * dx + dy * dy).sqrt();
    if distance < 1e-6 {
        return point;
    }
    let step = LINE_ANGLE_STEP_DEGREES.to_radians();
    let angle = (dy.atan2(dx) / step).round() * step;
    (start.0 + distance * angle.cos(), start.1 + distance * angle.sin())
}

/// The axis-aligned bounding box of a set of raw canvas-space points, plus
/// a normalizer that converts any point in that space into a `PathPoint`
/// fraction of the box — the shared conversion `Line` and `Path` creation
/// both need to turn a freehand drag into `Geometry` + local-space points.
/// A degenerate (zero-width or zero-height) box normalizes that axis to
/// `0.5` rather than dividing by zero.
fn normalize_points(points: &[(f64, f64)]) -> (Geometry, Vec<PathPoint>) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for &(x, y) in points {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    let width = max_x - min_x;
    let height = max_y - min_y;
    let geometry = Geometry::new(min_x, min_y, width, height);
    let normalized = points
        .iter()
        .map(|&(x, y)| {
            let fx = if width > 0.0 { (x - min_x) / width } else { 0.5 };
            let fy = if height > 0.0 { (y - min_y) / height } else { 0.5 };
            PathPoint::new(fx, fy)
        })
        .collect();
    (geometry, normalized)
}

/// Expands a selection into the actual set of objects that should move
/// together: any selected group is replaced by its descendants (a group
/// has no shape of its own), everything else passes through unchanged.
fn move_targets(document: &DesignDocument, ids: &BTreeSet<ObjectId>) -> Vec<ObjectId> {
    let mut result = Vec::new();
    for &id in ids {
        if document.get(id).map(|o| o.kind.is_group()).unwrap_or(false) {
            result.extend(document.descendants_of(id));
        } else {
            result.push(id);
        }
    }
    result.sort_unstable();
    result.dedup();
    result
}

/// Bounding box of a set of (id, geometry) pairs, e.g. a selection's
/// candidate positions mid-drag before they've been written to the
/// document. Assumes `items` is non-empty.
fn bbox_of(items: &[(ObjectId, Geometry)]) -> Geometry {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (_, g) in items {
        min_x = min_x.min(g.x);
        min_y = min_y.min(g.y);
        max_x = max_x.max(g.x + g.width);
        max_y = max_y.max(g.y + g.height);
    }
    Geometry::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

/// What the mouse is currently doing on the canvas. Geometry snapshots
/// (`origin`) hold what the moved/resized object(s) had when the drag
/// started, so each mousemove recomputes from that fixed baseline instead
/// of compounding small deltas.
#[derive(Clone, PartialEq)]
enum PointerAction {
    CreateDraft(DragState),
    /// Rubber-band selection drag on empty canvas.
    Marquee(DragState),
    /// An in-progress Pencil stroke: every recorded point so far, in raw
    /// canvas coordinates (converted to a normalized `Path` only once, on
    /// pointer-up — see `normalize_points`). Points are appended in
    /// `on_pointer_move`, but only when the pointer has moved at least
    /// `freehand::MIN_POINT_DISTANCE`, so a slow drag doesn't flood this
    /// with near-duplicates.
    DrawPath(Vec<(f64, f64)>),
    MoveSelection {
        origins: Vec<(ObjectId, Geometry)>,
        start_x: f64,
        start_y: f64,
        /// The specific object the drag started on, so a plain click (no
        /// real movement) on one member of a kept multi-selection can
        /// narrow the selection down to just that object on release.
        clicked_id: ObjectId,
    },
    ResizeObject { id: ObjectId, origin: Geometry, handle: HandleKind },
}

/// The largest zoom at which a `canvas_width` × `canvas_height` document
/// fits entirely inside a `workspace_width` × `workspace_height` area, with
/// `FIT_MARGIN` of breathing room on every side. Pure so it's easy to
/// reason about/test independently of the DOM measurement that feeds it.
fn compute_fit_zoom(workspace_width: f64, workspace_height: f64, canvas_width: f64, canvas_height: f64) -> f64 {
    let available_width = (workspace_width - FIT_MARGIN * 2.0).max(1.0);
    let available_height = (workspace_height - FIT_MARGIN * 2.0).max(1.0);
    (available_width / canvas_width).min(available_height / canvas_height).clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Maps a mouse event's viewport coordinates onto the SVG's own coordinate
/// space, regardless of which nested element the event actually targeted.
/// `view_width`/`view_height` are the document's actual canvas dimensions —
/// the SVG's `viewBox` always matches them exactly (see `canvas_area`), so
/// this conversion stays correct at any canvas size.
fn canvas_point(svg_ref: &NodeRef, event: &MouseEvent, view_width: f64, view_height: f64) -> Option<(f64, f64)> {
    let el = svg_ref.cast::<Element>()?;
    let rect = el.get_bounding_client_rect();
    if rect.width() == 0.0 || rect.height() == 0.0 {
        return None;
    }
    let scale_x = view_width / rect.width();
    let scale_y = view_height / rect.height();
    let x = (event.client_x() as f64 - rect.left()) * scale_x;
    let y = (event.client_y() as f64 - rect.top()) * scale_y;
    Some((x.clamp(0.0, view_width), y.clamp(0.0, view_height)))
}

/// An image object's rendered content: a real, still-vector `<image>`
/// element (never rasterizing the design — only this one bitmap paints as
/// a bitmap, same as any other embedded image in an SVG), clipped to its
/// box so `Crop` fit can't visually overflow it. Falls back to a dashed
/// placeholder if the asset it references is missing.
fn render_image_content(props: &ImageProperties, g: &Geometry, id: ObjectId, document: &DesignDocument) -> Html {
    let Some(asset) = document.assets.get(props.asset_id) else {
        return html! {
            <>
                <rect x="0" y="0" width={g.width.to_string()} height={g.height.to_string()} class="canvas-object__image-placeholder" />
                <text x={(g.width / 2.0).to_string()} y={(g.height / 2.0).to_string()} dominant-baseline="middle" text-anchor="middle" class="canvas-object__image-label">
                    {"▧"}
                </text>
            </>
        };
    };
    let preserve_aspect_ratio = match props.fit {
        ImageFit::Fill => "none",
        ImageFit::Fit => "xMidYMid meet",
        ImageFit::Crop => "xMidYMid slice",
    };
    let clip_id = format!("image-clip-{id}");

    html! {
        <>
            <defs>
                <clipPath id={clip_id.clone()}>
                    <rect x="0" y="0" width={g.width.to_string()} height={g.height.to_string()} />
                </clipPath>
            </defs>
            <image
                href={asset.reference.clone()}
                x="0" y="0"
                width={g.width.to_string()} height={g.height.to_string()}
                preserveAspectRatio={preserve_aspect_ratio}
                clip-path={format!("url(#{clip_id})")}
            />
        </>
    }
}

/// A text object's rendered content: real multiline SVG `<text>`/`<tspan>`s
/// (never rasterized, so it stays selectable and vector), positioned per
/// `text_align` and wrapped to `g.width` for `Fixed`-size boxes. `Auto`-size
/// text isn't wrapped — its box already grew to fit each line.
fn render_text_content(props: &TextProperties, g: &Geometry) -> Html {
    let lines: Vec<String> = match props.size_mode {
        TextSizeMode::Auto => props.content.split('\n').map(str::to_string).collect(),
        TextSizeMode::Fixed => text_metrics::wrap_lines(props, g.width),
    };
    let line_height_px = props.font_size * props.line_height;
    let (x, anchor) = match props.text_align {
        TextAlign::Left => (text_metrics::HORIZONTAL_PADDING, "start"),
        TextAlign::Center => (g.width / 2.0, "middle"),
        TextAlign::Right => (g.width - text_metrics::HORIZONTAL_PADDING, "end"),
    };
    let style = format!(
        "font-family:{};font-weight:{};font-style:{};letter-spacing:{}px;text-decoration:{};fill:{};",
        props.font_family.css_stack(),
        props.font_weight.css_value(),
        props.font_style.css_value(),
        props.letter_spacing,
        props.text_decoration.css_value(),
        props.fill.to_css(),
    );

    html! {
        <>
            <rect x="0" y="0" width={g.width.to_string()} height={g.height.to_string()} fill="transparent" />
            <text x={x.to_string()} y="0" text-anchor={anchor} font-size={props.font_size.to_string()} {style}>
                { for lines.iter().enumerate().map(|(i, line)| html! {
                    <tspan key={i} x={x.to_string()} y={(text_metrics::VERTICAL_PADDING + (i as f64 + 0.8) * line_height_px).to_string()}>
                        { line.clone() }
                    </tspan>
                }) }
            </text>
        </>
    }
}

/// How much wider than its visible stroke a line/path's invisible
/// click-target gets — a 1-2px stroke is otherwise nearly impossible to
/// click precisely, especially once zoomed out. Purely a hit-testing aid
/// (`pointer-events="stroke"` + `stroke="transparent"`); it paints nothing.
const HIT_AREA_PADDING: f64 = 10.0;

/// A line's rendered content: a real SVG `<line>` between its two
/// endpoints, converted from their local-space fractions (see
/// `model.rs`'s `PathPoint` docs) into the object's own `0..width,
/// 0..height` coordinate space — the same space `render_object`'s
/// translate+rotate wrapper already positions every other kind in. Paired
/// with an invisible, wider sibling so the line stays selectable without
/// needing pixel-precise clicks (see `HIT_AREA_PADDING`) — it's a sibling
/// rather than a wider visible stroke so it never changes what's drawn.
fn render_line_content(props: &LineProperties, g: &Geometry) -> Html {
    let x1 = (props.start.x * g.width).to_string();
    let y1 = (props.start.y * g.height).to_string();
    let x2 = (props.end.x * g.width).to_string();
    let y2 = (props.end.y * g.height).to_string();
    let style = format!("stroke:{};stroke-width:{};stroke-linecap:round;", props.stroke_color.to_css(), props.stroke_width);
    let hit_width = props.stroke_width + HIT_AREA_PADDING;
    html! {
        <>
            <line x1={x1.clone()} y1={y1.clone()} x2={x2.clone()} y2={y2.clone()} stroke="transparent" stroke-width={hit_width.to_string()} pointer-events="stroke" />
            <line {x1} {y1} {x2} {y2} {style} />
        </>
    }
}

/// A freehand path's rendered content: the stored local-space points
/// (already simplified at creation time — see `freehand.rs`) scaled into
/// the object's box, then smoothed into a single `<path>` `d` string.
/// Never filled. Paired with an invisible, wider hit-target sibling — see
/// `render_line_content`'s docs on `HIT_AREA_PADDING`.
fn render_path_content(props: &PathProperties, g: &Geometry) -> Html {
    let local_points: Vec<(f64, f64)> = props.points.iter().map(|p| (p.x * g.width, p.y * g.height)).collect();
    let d = freehand::smooth_path_d(&local_points);
    let hit_width = props.stroke_width + HIT_AREA_PADDING;
    let style = format!(
        "fill:none;stroke:{};stroke-width:{};stroke-linecap:round;stroke-linejoin:round;",
        props.stroke_color.to_css(),
        props.stroke_width
    );
    html! {
        <>
            <path d={d.clone()} fill="none" stroke="transparent" stroke-width={hit_width.to_string()} pointer-events="stroke" />
            <path {d} {style} />
        </>
    }
}

/// Inline `fill`/`stroke` for a rectangle/ellipse, built from its
/// `ShapeProperties` — overrides the CSS class's placeholder fill/stroke
/// (see `.canvas-object__rect`/`.canvas-object__ellipse` in styles.css) now
/// that shapes carry real per-object color data.
fn shape_style(props: &ShapeProperties) -> String {
    let stroke = if props.stroke.enabled {
        format!("stroke:{};stroke-width:{};", props.stroke.color.to_css(), props.stroke.width)
    } else {
        "stroke:none;".to_string()
    };
    format!("fill:{};{stroke}", props.fill.to_css())
}

/// Renders one object's shape, positioned via a translate+rotate transform
/// so every kind shares the same rotation/opacity/hit-testing wiring. A
/// group has no shape of its own — only its children (separate objects in
/// the same flat list) render anything. A text object currently in inline
/// edit mode also renders nothing here — the editing textarea overlay is
/// its sole visible/interactive surface until the session ends.
#[allow(clippy::too_many_arguments)]
fn render_object(
    object: &DesignObject,
    document: &DesignDocument,
    onmousedown: Callback<MouseEvent>,
    ondblclick: Callback<MouseEvent>,
    is_selected: bool,
    is_editing: bool,
) -> Html {
    if object.kind.is_group() || is_editing {
        return html! {};
    }

    let g = &object.geometry;
    let transform = format!(
        "translate({} {}) rotate({} {} {})",
        g.x,
        g.y,
        g.rotation,
        g.width / 2.0,
        g.height / 2.0
    );
    let class = if is_selected {
        "canvas-object canvas-object--selected"
    } else {
        "canvas-object"
    };

    let inner = match &object.kind {
        ObjectKind::Rectangle(props) => html! {
            <rect x="0" y="0" width={g.width.to_string()} height={g.height.to_string()} class="canvas-object__rect" style={shape_style(props)} />
        },
        ObjectKind::Ellipse(props) => html! {
            <ellipse
                cx={(g.width / 2.0).to_string()}
                cy={(g.height / 2.0).to_string()}
                rx={(g.width / 2.0).to_string()}
                ry={(g.height / 2.0).to_string()}
                class="canvas-object__ellipse"
                style={shape_style(props)}
            />
        },
        ObjectKind::Line(props) => render_line_content(props, g),
        ObjectKind::Path(props) => render_path_content(props, g),
        ObjectKind::Text(props) => render_text_content(props, g),
        ObjectKind::Image(props) => render_image_content(props, g, object.id, document),
        ObjectKind::Group => html! {},
    };

    html! {
        <g key={object.id} {transform} opacity={g.opacity.to_string()} {onmousedown} {ondblclick} {class}>
            { inner }
        </g>
    }
}

/// Outline + 8 resize handles around a single selected (non-group) object.
fn render_single_selection(object: &DesignObject, on_handle_down: Callback<(ObjectId, HandleKind, MouseEvent)>) -> Html {
    let id = object.id;
    let g = &object.geometry;
    let transform = format!(
        "translate({} {}) rotate({} {} {})",
        g.x,
        g.y,
        g.rotation,
        g.width / 2.0,
        g.height / 2.0
    );

    html! {
        <g {transform} class="selection-box">
            <rect x="-1" y="-1" width={(g.width + 2.0).to_string()} height={(g.height + 2.0).to_string()} class="selection-box__outline" />
            { for HandleKind::ALL.iter().map(|&handle| {
                let (hx, hy) = handle.local_position(g.width, g.height);
                let on_handle_down = on_handle_down.clone();
                let onmousedown = Callback::from(move |event: MouseEvent| {
                    event.stop_propagation();
                    on_handle_down.emit((id, handle, event));
                });
                html! {
                    <rect
                        x={(hx - HANDLE_SIZE / 2.0).to_string()}
                        y={(hy - HANDLE_SIZE / 2.0).to_string()}
                        width={HANDLE_SIZE.to_string()}
                        height={HANDLE_SIZE.to_string()}
                        class="selection-box__handle"
                        style={format!("cursor: {}", handle.cursor())}
                        {onmousedown}
                    />
                }
            }) }
        </g>
    }
}

/// Outline-only combined bounding box for a multi-object or group
/// selection — no resize handles, since resizing a group/multi-selection
/// as a unit isn't implemented.
fn render_combined_selection(bbox: &Geometry) -> Html {
    html! {
        <rect
            x={(bbox.x - 1.0).to_string()}
            y={(bbox.y - 1.0).to_string()}
            width={(bbox.width + 2.0).to_string()}
            height={(bbox.height + 2.0).to_string()}
            class="selection-box__outline selection-box__outline--combined"
        />
    }
}

fn render_draft(drag: &DragState, tool: Tool) -> Html {
    let g = drag.geometry();
    match tool {
        Tool::Rectangle => html! {
            <rect x={g.x.to_string()} y={g.y.to_string()} width={g.width.to_string()} height={g.height.to_string()} class="canvas-draft" />
        },
        Tool::Ellipse => html! {
            <ellipse
                cx={(g.x + g.width / 2.0).to_string()}
                cy={(g.y + g.height / 2.0).to_string()}
                rx={(g.width / 2.0).to_string()}
                ry={(g.height / 2.0).to_string()}
                class="canvas-draft"
            />
        },
        Tool::Line => html! {
            <line
                x1={drag.start_x.to_string()} y1={drag.start_y.to_string()}
                x2={drag.current_x.to_string()} y2={drag.current_y.to_string()}
                class="canvas-draft"
            />
        },
        _ => html! {},
    }
}

/// Live preview of an in-progress Pencil stroke — the same smoothing the
/// finished `Path` renders with, applied directly to the raw (not yet
/// simplified) recorded points, so the preview never jumps when
/// simplification runs on pointer-up.
fn render_path_draft(points: &[(f64, f64)]) -> Html {
    let d = freehand::smooth_path_d(points);
    html! { <path {d} class="canvas-draft" style="fill:none;" /> }
}

/// Temporary alignment guide lines shown only while an object is actively
/// being dragged into alignment — never persisted, rebuilt fresh each
/// pointer move, and rendered as plain `<line>`s so nothing exists in the
/// SVG when there's nothing to show.
fn render_guides(guides: &[Guide]) -> Html {
    html! {
        <>
            { for guides.iter().map(|guide| match guide.orientation {
                GuideOrientation::Vertical => html! {
                    <line
                        x1={guide.position.to_string()} y1={guide.start.to_string()}
                        x2={guide.position.to_string()} y2={guide.end.to_string()}
                        class="smart-guide"
                    />
                },
                GuideOrientation::Horizontal => html! {
                    <line
                        x1={guide.start.to_string()} y1={guide.position.to_string()}
                        x2={guide.end.to_string()} y2={guide.position.to_string()}
                        class="smart-guide"
                    />
                },
            }) }
        </>
    }
}

fn render_marquee(drag: &DragState) -> Html {
    let g = drag.geometry();
    html! {
        <rect x={g.x.to_string()} y={g.y.to_string()} width={g.width.to_string()} height={g.height.to_string()} class="marquee-select" />
    }
}

/// The live-editing surface for a text object: a plain `<textarea>` — not
/// `contentEditable` (see module docs for why) — absolutely positioned over
/// the object's on-canvas box and styled to match its typography, so
/// switching in and out of edit mode doesn't visibly jump. Every keystroke
/// syncs straight back into the document via `UpdateTextProperties`
/// (unrecorded); nothing here is itself the source of truth. `scale_x`/
/// `scale_y` convert the object's SVG-space geometry to on-screen CSS
/// pixels, matching whatever size the SVG is actually rendered at.
#[allow(clippy::too_many_arguments)]
fn render_text_editor(
    editor: &EditorContext,
    id: ObjectId,
    props: &TextProperties,
    g: &Geometry,
    scale_x: f64,
    scale_y: f64,
    text_area_ref: NodeRef,
    editing_snapshot: UseStateHandle<Option<(ObjectId, TextProperties, Geometry)>>,
) -> Html {
    let align = match props.text_align {
        TextAlign::Left => "left",
        TextAlign::Center => "center",
        TextAlign::Right => "right",
    };
    let style = format!(
        "position:absolute; left:{}px; top:{}px; width:{}px; height:{}px; \
         font-family:{}; font-size:{}px; font-weight:{}; font-style:{}; \
         line-height:{}; letter-spacing:{}px; text-align:{align}; color:{}; \
         padding:{}px {}px;",
        g.x * scale_x,
        g.y * scale_y,
        g.width * scale_x,
        g.height * scale_y,
        props.font_family.css_stack(),
        props.font_size * scale_y,
        props.font_weight.css_value(),
        props.font_style.css_value(),
        props.line_height,
        props.letter_spacing * scale_x,
        props.fill.to_css(),
        text_metrics::VERTICAL_PADDING * scale_y,
        text_metrics::HORIZONTAL_PADDING * scale_x,
    );

    let commit = {
        let editor = editor.clone();
        let editing_snapshot = editing_snapshot.clone();
        Callback::from(move |()| {
            if let Some((snapshot_id, before, before_geometry)) = (*editing_snapshot).clone() {
                if snapshot_id == id {
                    editor.dispatch(EditorAction::CommitTextEdit { id, before, before_geometry });
                }
            }
        })
    };

    let oninput = {
        let editor = editor.clone();
        Callback::from(move |event: InputEvent| {
            let textarea: HtmlTextAreaElement = event.target_unchecked_into();
            let content = textarea.value();
            if let Some(object) = editor.document.get(id) {
                if let ObjectKind::Text(current) = &object.kind {
                    let mut updated = current.clone();
                    updated.content = content;
                    editor.dispatch(EditorAction::UpdateTextProperties { id, properties: updated });
                }
            }
        })
    };

    let onblur = {
        let commit = commit.clone();
        Callback::from(move |_: FocusEvent| commit.emit(()))
    };

    let onkeydown = {
        let editor = editor.clone();
        Callback::from(move |event: KeyboardEvent| {
            // Editing text owns every keystroke here — arrow-key nudge,
            // Delete-selected-object, Ctrl+Z undo, etc. must not also fire
            // on the design document while the user is just typing.
            event.stop_propagation();
            match event.key().as_str() {
                "Escape" => {
                    event.prevent_default();
                    commit.emit(());
                }
                key if (event.ctrl_key() || event.meta_key()) && key.eq_ignore_ascii_case("b") => {
                    event.prevent_default();
                    if let Some(object) = editor.document.get(id) {
                        if let ObjectKind::Text(current) = &object.kind {
                            let mut updated = current.clone();
                            updated.font_weight = if updated.font_weight == FontWeight::Bold { FontWeight::Regular } else { FontWeight::Bold };
                            editor.dispatch(EditorAction::UpdateTextProperties { id, properties: updated });
                        }
                    }
                }
                key if (event.ctrl_key() || event.meta_key()) && key.eq_ignore_ascii_case("i") => {
                    event.prevent_default();
                    if let Some(object) = editor.document.get(id) {
                        if let ObjectKind::Text(current) = &object.kind {
                            let mut updated = current.clone();
                            updated.font_style = if updated.font_style == FontStyle::Italic { FontStyle::Regular } else { FontStyle::Italic };
                            editor.dispatch(EditorAction::UpdateTextProperties { id, properties: updated });
                        }
                    }
                }
                _ => {}
            }
        })
    };

    html! {
        <textarea
            ref={text_area_ref}
            class="text-edit-overlay"
            {style}
            value={props.content.clone()}
            {oninput}
            {onkeydown}
            {onblur}
        />
    }
}

#[function_component(CanvasArea)]
pub fn canvas_area() -> Html {
    let editor = use_editor();
    let svg_ref = use_node_ref();
    let canvas_area_ref = use_node_ref();
    let pointer_action = use_state(|| None::<PointerAction>);
    let guides = use_state(Vec::<Guide>::new);
    let text_area_ref = use_node_ref();
    // The text object's properties/geometry at the moment its edit session
    // started, so `CommitTextEdit` can diff against the (already live)
    // document to build one undo step covering the whole session.
    let editing_snapshot = use_state(|| None::<(ObjectId, TextProperties, Geometry)>);
    // Whether a file is currently being dragged over the canvas — purely a
    // transient visual cue, never touches the document.
    let is_drag_over = use_state(|| false);
    // Bumped on every window "resize" event (and once on mount) purely to
    // force a re-render — a resize alone changes no Yew state, but Fit mode
    // needs to re-measure `canvas_area_ref` and recompute its zoom when the
    // workspace actually changes size.
    let resize_tick = use_state(|| 0u32);

    // Registers a window resize listener for the component's lifetime. The
    // listener `Closure` is moved into the returned cleanup closure so it
    // stays alive for as long as it's registered — dropping it earlier
    // would invalidate the JS callback while the browser can still call it.
    {
        let resize_tick = resize_tick.clone();
        use_effect_with((), move |()| {
            // Forces one extra render right after mount, once
            // `canvas_area_ref` is actually attached to the DOM, so Fit
            // mode's first measurement reflects real layout instead of the
            // default zoom.
            resize_tick.set(1);

            let tick_for_listener = resize_tick.clone();
            let closure = Closure::<dyn Fn()>::new(move || {
                tick_for_listener.set((*tick_for_listener).wrapping_add(1));
            });
            let window = web_sys::window();
            if let Some(win) = window.as_ref() {
                let _ = win.add_event_listener_with_callback("resize", closure.as_ref().unchecked_ref());
            }
            move || {
                if let Some(win) = window {
                    let _ = win.remove_event_listener_with_callback("resize", closure.as_ref().unchecked_ref());
                }
                drop(closure);
            }
        });
    }

    {
        let editing_snapshot = editing_snapshot.clone();
        let editor = editor.clone();
        let text_area_ref = text_area_ref.clone();
        use_effect_with(editor.editing_text, move |editing_text| {
            match *editing_text {
                Some(id) => {
                    if let Some(object) = editor.document.get(id) {
                        if let ObjectKind::Text(props) = &object.kind {
                            editing_snapshot.set(Some((id, props.clone(), object.geometry)));
                        }
                    }
                    if let Some(el) = text_area_ref.cast::<HtmlTextAreaElement>() {
                        let _ = el.focus();
                    }
                }
                None => editing_snapshot.set(None),
            }
            || ()
        });
    }

    // Fit mode: while it's on, keep `editor.zoom` in sync with the largest
    // zoom that fits the whole document inside the actual measured
    // workspace (`canvas_area_ref`'s own box, which — thanks to the grid
    // layout in `styles.css` — already excludes the toolbar/left panel/
    // right panel/status bar). Re-runs when fit mode toggles on, the
    // canvas is resized, or the window is (`resize_tick`); the dispatch
    // only fires when the computed value actually moved, so this settles
    // after one extra render instead of looping.
    {
        let editor = editor.clone();
        let canvas_area_ref = canvas_area_ref.clone();
        let fit_mode = editor.fit_mode;
        let canvas_width = editor.document.canvas_width;
        let canvas_height = editor.document.canvas_height;
        let current_zoom = editor.zoom;
        use_effect_with((fit_mode, canvas_width, canvas_height, *resize_tick), move |_| {
            if fit_mode {
                if let Some(rect) = canvas_area_ref.cast::<Element>().map(|el| el.get_bounding_client_rect()) {
                    if rect.width() > 0.0 && rect.height() > 0.0 {
                        let target = compute_fit_zoom(rect.width(), rect.height(), canvas_width, canvas_height);
                        if (target - current_zoom).abs() > FIT_ZOOM_EPSILON {
                            editor.dispatch(EditorAction::SyncFitZoom(target));
                        }
                    }
                }
            }
            || ()
        });
    }

    let on_pointer_down = {
        let editor = editor.clone();
        let svg_ref = svg_ref.clone();
        let pointer_action = pointer_action.clone();
        let guides = guides.clone();
        Callback::from(move |event: MouseEvent| {
            guides.set(Vec::new());
            let Some((x, y)) = canvas_point(&svg_ref, &event, editor.document.canvas_width, editor.document.canvas_height) else {
                return;
            };
            match editor.active_tool {
                Tool::Rectangle | Tool::Ellipse | Tool::Line => {
                    pointer_action.set(Some(PointerAction::CreateDraft(DragState {
                        start_x: x,
                        start_y: y,
                        current_x: x,
                        current_y: y,
                    })));
                }
                Tool::Pen => {
                    pointer_action.set(Some(PointerAction::DrawPath(vec![(x, y)])));
                }
                Tool::Text => {
                    // The click point becomes the new box's exact
                    // top-left corner, right on the boundary of where the
                    // browser's own default mousedown focus handling
                    // would land — left unchecked, that races the
                    // autofocus this triggers on the fresh edit-mode
                    // textarea and can blur it (and delete the still-empty
                    // text) before a keystroke ever lands.
                    event.prevent_default();
                    let properties = TextProperties::default();
                    let (width, height) = text_metrics::auto_size(&properties);
                    editor.dispatch(EditorAction::CreateObject {
                        kind: ObjectKind::Text(properties),
                        geometry: Geometry::new(x, y, width, height),
                    });
                }
                Tool::Select => {
                    pointer_action.set(Some(PointerAction::Marquee(DragState {
                        start_x: x,
                        start_y: y,
                        current_x: x,
                        current_y: y,
                    })));
                }
                Tool::Hand => {}
            }
        })
    };

    let on_pointer_move = {
        let svg_ref = svg_ref.clone();
        let pointer_action = pointer_action.clone();
        let editor = editor.clone();
        let guides = guides.clone();
        Callback::from(move |event: MouseEvent| {
            let Some(action) = (*pointer_action).clone() else {
                return;
            };
            let Some((x, y)) = canvas_point(&svg_ref, &event, editor.document.canvas_width, editor.document.canvas_height) else {
                return;
            };
            match action {
                PointerAction::CreateDraft(state) => {
                    let (current_x, current_y) = if editor.active_tool == Tool::Line && event.shift_key() {
                        constrain_line_angle((state.start_x, state.start_y), (x, y))
                    } else {
                        (x, y)
                    };
                    pointer_action.set(Some(PointerAction::CreateDraft(DragState {
                        current_x,
                        current_y,
                        ..state
                    })));
                }
                PointerAction::DrawPath(mut points) => {
                    let moved_enough = points.last().is_none_or(|&(lx, ly)| ((x - lx).powi(2) + (y - ly).powi(2)).sqrt() >= freehand::MIN_POINT_DISTANCE);
                    if moved_enough {
                        points.push((x, y));
                        pointer_action.set(Some(PointerAction::DrawPath(points)));
                    }
                }
                PointerAction::Marquee(state) => {
                    pointer_action.set(Some(PointerAction::Marquee(DragState {
                        current_x: x,
                        current_y: y,
                        ..state
                    })));
                }
                PointerAction::MoveSelection { origins, start_x, start_y, .. } => {
                    let dx = x - start_x;
                    let dy = y - start_y;
                    let candidates: Vec<(ObjectId, Geometry)> = origins
                        .iter()
                        .map(|(id, origin)| (*id, Geometry { x: origin.x + dx, y: origin.y + dy, ..*origin }))
                        .collect();
                    let exclude: BTreeSet<ObjectId> = origins.iter().map(|(id, _)| *id).collect();
                    let (snap_dx, snap_dy, new_guides) =
                        compute_snap(&editor.document, &exclude, bbox_of(&candidates), editor.document.canvas_width, editor.document.canvas_height);
                    guides.set(new_guides);
                    for (id, origin) in &origins {
                        let geometry = Geometry { x: origin.x + dx + snap_dx, y: origin.y + dy + snap_dy, ..*origin };
                        editor.dispatch(EditorAction::UpdateGeometry { id: *id, geometry });
                    }
                }
                PointerAction::ResizeObject { id, origin, handle } => {
                    let mut geometry = compute_resized_geometry(origin, handle, x, y);
                    if event.shift_key() {
                        geometry = apply_aspect_lock(origin, handle, geometry);
                    }
                    editor.dispatch(EditorAction::UpdateGeometry { id, geometry });
                }
            }
        })
    };

    let on_pointer_up = {
        let editor = editor.clone();
        let pointer_action = pointer_action.clone();
        let svg_ref = svg_ref.clone();
        let guides = guides.clone();
        Callback::from(move |event: MouseEvent| {
            guides.set(Vec::new());
            if let Some(action) = (*pointer_action).clone() {
                match action {
                    PointerAction::CreateDraft(state) if editor.active_tool == Tool::Line => {
                        let start = (state.start_x, state.start_y);
                        let end = (state.current_x, state.current_y);
                        let length = ((end.0 - start.0).powi(2) + (end.1 - start.1).powi(2)).sqrt();
                        if length >= MIN_DRAG_SIZE {
                            let (geometry, normalized) = normalize_points(&[start, end]);
                            let properties = LineProperties { start: normalized[0], end: normalized[1], ..LineProperties::default() };
                            editor.dispatch(EditorAction::CreateObject { kind: ObjectKind::Line(properties), geometry });
                        }
                    }
                    PointerAction::CreateDraft(state) => {
                        let geometry = state.geometry();
                        if geometry.width >= MIN_DRAG_SIZE && geometry.height >= MIN_DRAG_SIZE {
                            let kind = match editor.active_tool {
                                Tool::Ellipse => ObjectKind::Ellipse(ShapeProperties::default()),
                                _ => ObjectKind::Rectangle(ShapeProperties::default()),
                            };
                            editor.dispatch(EditorAction::CreateObject { kind, geometry });
                        }
                    }
                    PointerAction::DrawPath(points) => {
                        let simplified = freehand::simplify(&points, freehand::SIMPLIFY_EPSILON);
                        if simplified.len() >= 2 {
                            let (geometry, normalized) = normalize_points(&simplified);
                            let properties = PathProperties { points: normalized, ..PathProperties::default() };
                            editor.dispatch(EditorAction::CreateObject { kind: ObjectKind::Path(properties), geometry });
                        }
                    }
                    PointerAction::Marquee(state) => {
                        let g = state.geometry();
                        let shift = event.shift_key();
                        if g.width < MIN_DRAG_SIZE && g.height < MIN_DRAG_SIZE {
                            if !shift {
                                editor.dispatch(EditorAction::SetSelection(BTreeSet::new()));
                            }
                        } else {
                            let hit: BTreeSet<ObjectId> = editor
                                .document
                                .objects
                                .iter()
                                .filter(|o| !o.kind.is_group())
                                .filter(|o| {
                                    let og = &o.geometry;
                                    og.x >= g.x && og.y >= g.y && og.x + og.width <= g.x + g.width && og.y + og.height <= g.y + g.height
                                })
                                .map(|o| editor.document.outermost_ancestor(o.id))
                                .collect();
                            let selection = if shift {
                                editor.selected_ids.union(&hit).copied().collect()
                            } else {
                                hit
                            };
                            editor.dispatch(EditorAction::SetSelection(selection));
                        }
                    }
                    PointerAction::MoveSelection { origins, start_x, start_y, clicked_id } => {
                        if let Some((x, y)) = canvas_point(&svg_ref, &event, editor.document.canvas_width, editor.document.canvas_height) {
                            let dx = x - start_x;
                            let dy = y - start_y;
                            let moved = dx.abs() >= MIN_DRAG_SIZE || dy.abs() >= MIN_DRAG_SIZE;
                            if !moved && !event.shift_key() && editor.selected_ids.len() > 1 {
                                // A plain click (no drag) on one member of a
                                // kept multi-selection narrows to just it.
                                editor.dispatch(EditorAction::SetSelection(BTreeSet::from([clicked_id])));
                            } else if moved {
                                // Read back from the document rather than
                                // recomputing from the raw pointer delta, so
                                // the commit keeps whatever snap offset the
                                // last pointer move already applied.
                                let changes: Vec<(ObjectId, Geometry, Geometry)> = origins
                                    .iter()
                                    .filter_map(|(id, origin)| editor.document.get(*id).map(|o| (*id, *origin, o.geometry)))
                                    .collect();
                                editor.dispatch(EditorAction::CommitGeometries(changes));
                            }
                        }
                    }
                    PointerAction::ResizeObject { id, origin, .. } => {
                        if let Some(object) = editor.document.get(id) {
                            // Manually resizing an Auto-size text box is
                            // the signal that the user wants to control
                            // its size from now on, so it switches to
                            // Fixed (wrapping) as part of the same commit.
                            if let ObjectKind::Text(props) = &object.kind {
                                if props.size_mode == TextSizeMode::Auto {
                                    let mut after_props = props.clone();
                                    after_props.size_mode = TextSizeMode::Fixed;
                                    editor.dispatch(EditorAction::CommitTextResize {
                                        id,
                                        before_geometry: origin,
                                        before_props: props.clone(),
                                        after_props,
                                    });
                                } else {
                                    editor.dispatch(EditorAction::CommitGeometries(vec![(id, origin, object.geometry)]));
                                }
                            } else {
                                editor.dispatch(EditorAction::CommitGeometries(vec![(id, origin, object.geometry)]));
                            }
                        }
                    }
                }
            }
            pointer_action.set(None);
        })
    };

    let on_handle_down = {
        let editor = editor.clone();
        let pointer_action = pointer_action.clone();
        Callback::from(move |(id, handle, _event): (ObjectId, HandleKind, MouseEvent)| {
            if editor.active_tool != Tool::Select {
                return;
            }
            let Some(object) = editor.document.get(id) else {
                return;
            };
            pointer_action.set(Some(PointerAction::ResizeObject {
                id,
                origin: object.geometry,
                handle,
            }));
        })
    };

    let resolved_ids = move_targets(&editor.document, &editor.selected_ids);
    let single_selected_id = (editor.selected_ids.len() == 1)
        .then(|| *editor.selected_ids.iter().next().unwrap())
        .filter(|id| editor.document.get(*id).map(|o| !o.kind.is_group()).unwrap_or(false));

    // While a text object is being edited, its textarea overlay is the
    // only interactive surface for it — resize handles would just
    // conflict with typing.
    let selection_view = if editor.editing_text.is_some() {
        html! {}
    } else if let Some(id) = single_selected_id {
        editor.document.get(id).map(|object| render_single_selection(object, on_handle_down)).unwrap_or_else(|| html! {})
    } else if !resolved_ids.is_empty() {
        editor.document.bounding_box(&resolved_ids).map(|bbox| render_combined_selection(&bbox)).unwrap_or_else(|| html! {})
    } else {
        html! {}
    };

    // Converts the SVG's own document-canvas coordinate space to whatever
    // CSS pixel size it's actually rendered at, so the text-edit textarea
    // (a plain HTML element outside the SVG) lines up with it exactly.
    let (overlay_scale_x, overlay_scale_y) = svg_ref
        .cast::<Element>()
        .map(|el| {
            let rect = el.get_bounding_client_rect();
            if rect.width() > 0.0 && rect.height() > 0.0 {
                (rect.width() / editor.document.canvas_width, rect.height() / editor.document.canvas_height)
            } else {
                (1.0, 1.0)
            }
        })
        .unwrap_or((1.0, 1.0));

    let text_editor_overlay = match editor.editing_text.and_then(|id| editor.document.get(id).map(|o| (id, o))) {
        Some((id, object)) => match &object.kind {
            ObjectKind::Text(props) => render_text_editor(
                &editor,
                id,
                props,
                &object.geometry,
                overlay_scale_x,
                overlay_scale_y,
                text_area_ref.clone(),
                editing_snapshot.clone(),
            ),
            _ => html! {},
        },
        None => html! {},
    };

    // Native HTML5 file drag-and-drop — a separate event family from the
    // mousedown/move/up used for moving objects, so it can't interfere
    // with normal object dragging. `ondragover` must call
    // `prevent_default` or the browser refuses the drop outright.
    let on_drag_over = {
        let is_drag_over = is_drag_over.clone();
        Callback::from(move |event: DragEvent| {
            event.prevent_default();
            if !*is_drag_over {
                is_drag_over.set(true);
            }
        })
    };
    let on_drag_leave = {
        let is_drag_over = is_drag_over.clone();
        Callback::from(move |_: DragEvent| is_drag_over.set(false))
    };
    let on_drop = {
        let editor = editor.clone();
        let svg_ref = svg_ref.clone();
        let is_drag_over = is_drag_over.clone();
        Callback::from(move |event: DragEvent| {
            event.prevent_default();
            is_drag_over.set(false);
            // `DragEvent` extends `MouseEvent` in the DOM, so it carries
            // the same client coordinates `canvas_point` reads.
            let (canvas_width, canvas_height) = (editor.document.canvas_width, editor.document.canvas_height);
            let center = canvas_point(&svg_ref, &event, canvas_width, canvas_height).unwrap_or((canvas_width / 2.0, canvas_height / 2.0));
            for (index, file) in files_from_drop(&event).into_iter().enumerate() {
                let offset = index as f64 * DUPLICATE_OFFSET;
                import_image_file(
                    editor.clone(),
                    file,
                    ImportTarget::NewObject { center: (center.0 + offset, center.1 + offset) },
                );
            }
        })
    };

    // Ctrl/Cmd+wheel zooms, centered on the cursor; a plain wheel is left
    // alone entirely (no `prevent_default`) so normal scrolling/panning
    // keeps working exactly as before.
    let on_wheel = {
        let editor = editor.clone();
        let canvas_area_ref = canvas_area_ref.clone();
        Callback::from(move |event: WheelEvent| {
            if !(event.ctrl_key() || event.meta_key()) {
                return;
            }
            event.prevent_default();
            let old_zoom = editor.zoom;
            let factor = if event.delta_y() < 0.0 { 1.1 } else { 1.0 / 1.1 };
            let new_zoom = (old_zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
            if let Some(container) = canvas_area_ref.cast::<Element>() {
                let rect = container.get_bounding_client_rect();
                let cursor_x = event.client_x() as f64 - rect.left();
                let cursor_y = event.client_y() as f64 - rect.top();
                let ratio = new_zoom / old_zoom - 1.0;
                let new_scroll_left = container.scroll_left() as f64 + cursor_x * ratio;
                let new_scroll_top = container.scroll_top() as f64 + cursor_y * ratio;
                editor.dispatch(EditorAction::SetZoom(new_zoom));
                container.set_scroll_left(new_scroll_left as i32);
                container.set_scroll_top(new_scroll_top as i32);
            } else {
                editor.dispatch(EditorAction::SetZoom(new_zoom));
            }
        })
    };

    let artboard_class =
        classes!("canvas-area__artboard", is_drag_over.then_some("canvas-area__artboard--drag-over"));
    let canvas_width = editor.document.canvas_width;
    let canvas_height = editor.document.canvas_height;
    // The document's coordinate system never changes with zoom — the SVG
    // keeps its `viewBox` fixed at the actual document size. Only the
    // rendered CSS box (both the artboard chrome and the SVG itself) is
    // sized to `document size × zoom`: screenPosition = documentPosition ×
    // zoom. Sizing the box itself (rather than a `transform: scale()` on
    // top of a full-size box) is what keeps `.canvas-area`'s `overflow:
    // auto` scrollable region tied to the *visible* size — a full-size box
    // regardless of zoom is exactly what forced scrollbars at large canvas
    // sizes.
    let display_width = canvas_width * editor.zoom;
    let display_height = canvas_height * editor.zoom;
    // The background color is a document property (`DesignDocument::
    // background`) rather than the static `--bg-artboard` the CSS class
    // otherwise falls back to, so it's set here as an inline override.
    let artboard_style = format!("width: {display_width}px; height: {display_height}px; background-color: {};", editor.document.background);
    let svg_style = format!("width: {display_width}px; height: {display_height}px;");

    html! {
        <section class="canvas-area" tabindex="0" ref={canvas_area_ref.clone()} ondragover={on_drag_over} ondragleave={on_drag_leave} ondrop={on_drop} onwheel={on_wheel}>
            <div class={artboard_class} style={artboard_style}>
                <svg
                    ref={svg_ref.clone()}
                    class="canvas-area__svg"
                    style={svg_style}
                    viewBox={format!("0 0 {canvas_width} {canvas_height}")}
                    xmlns="http://www.w3.org/2000/svg"
                    onmousedown={on_pointer_down}
                    onmousemove={on_pointer_move}
                    onmouseup={on_pointer_up}
                >
                    { for editor.document.objects.iter().map(|object| {
                        let document = &editor.document;
                        let editor = editor.clone();
                        let svg_ref = svg_ref.clone();
                        let pointer_action = pointer_action.clone();
                        let id = object.id;
                        let is_selected = editor.is_selected(id);
                        let is_editing = editor.editing_text == Some(id);
                        let ondblclick = {
                            let editor = editor.clone();
                            let is_text = matches!(object.kind, ObjectKind::Text(_));
                            Callback::from(move |event: MouseEvent| {
                                if editor.active_tool != Tool::Select || !is_text {
                                    return;
                                }
                                event.stop_propagation();
                                event.prevent_default();
                                editor.dispatch(EditorAction::SetSelection(BTreeSet::from([id])));
                                editor.dispatch(EditorAction::SetEditingText(Some(id)));
                            })
                        };
                        let onmousedown = Callback::from(move |event: MouseEvent| {
                            if editor.active_tool != Tool::Select {
                                return;
                            }
                            event.stop_propagation();
                            let effective_id = editor.document.outermost_ancestor(id);
                            let mut selection = editor.selected_ids.clone();
                            if event.shift_key() {
                                if !selection.remove(&effective_id) {
                                    selection.insert(effective_id);
                                }
                                editor.dispatch(EditorAction::SetSelection(selection));
                                return;
                            }
                            if !selection.contains(&effective_id) || selection.len() == 1 {
                                selection = BTreeSet::from([effective_id]);
                                editor.dispatch(EditorAction::SetSelection(selection.clone()));
                            }
                            let targets = move_targets(&editor.document, &selection);
                            let Some((x, y)) = canvas_point(&svg_ref, &event, editor.document.canvas_width, editor.document.canvas_height) else {
                                return;
                            };
                            let origins: Vec<(ObjectId, Geometry)> =
                                targets.iter().filter_map(|id| editor.document.get(*id).map(|o| (*id, o.geometry))).collect();
                            if !origins.is_empty() {
                                pointer_action.set(Some(PointerAction::MoveSelection {
                                    origins,
                                    start_x: x,
                                    start_y: y,
                                    clicked_id: effective_id,
                                }));
                            }
                        });
                        render_object(object, document, onmousedown, ondblclick, is_selected, is_editing)
                    }) }
                    { match &*pointer_action {
                        Some(PointerAction::CreateDraft(state)) => render_draft(state, editor.active_tool),
                        Some(PointerAction::Marquee(state)) => render_marquee(state),
                        Some(PointerAction::DrawPath(points)) => render_path_draft(points),
                        _ => html! {},
                    } }
                    { selection_view }
                    { render_guides(&guides) }
                </svg>
                { text_editor_overlay }
            </div>
        </section>
    }
}
