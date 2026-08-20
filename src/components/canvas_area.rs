use std::collections::BTreeSet;

use web_sys::{Element, FocusEvent, HtmlTextAreaElement, InputEvent, MouseEvent};
use yew::prelude::*;

use crate::editor_state::{use_editor, EditorAction, EditorContext, Tool};
use crate::model::{
    DesignDocument, DesignObject, FontStyle, FontWeight, Geometry, ObjectId, ObjectKind, TextAlign, TextProperties, TextSizeMode,
    MIN_OBJECT_SIZE,
};
use crate::snapping::{compute_snap, Guide, GuideOrientation};
use crate::text_metrics;

const VIEW_WIDTH: f64 = 800.0;
const VIEW_HEIGHT: f64 = 600.0;
const MIN_DRAG_SIZE: f64 = 2.0;
const HANDLE_SIZE: f64 = 6.0;

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

/// Maps a mouse event's viewport coordinates onto the SVG's own coordinate
/// space, regardless of which nested element the event actually targeted.
fn canvas_point(svg_ref: &NodeRef, event: &MouseEvent) -> Option<(f64, f64)> {
    let el = svg_ref.cast::<Element>()?;
    let rect = el.get_bounding_client_rect();
    if rect.width() == 0.0 || rect.height() == 0.0 {
        return None;
    }
    let scale_x = VIEW_WIDTH / rect.width();
    let scale_y = VIEW_HEIGHT / rect.height();
    let x = (event.client_x() as f64 - rect.left()) * scale_x;
    let y = (event.client_y() as f64 - rect.top()) * scale_y;
    Some((x.clamp(0.0, VIEW_WIDTH), y.clamp(0.0, VIEW_HEIGHT)))
}

fn kind_icon(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Rectangle => "▭",
        ObjectKind::Ellipse => "○",
        ObjectKind::Text(_) => "T",
        ObjectKind::ImagePlaceholder => "▨",
        ObjectKind::Group => "▤",
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
        props.fill,
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

/// Renders one object's shape, positioned via a translate+rotate transform
/// so every kind shares the same rotation/opacity/hit-testing wiring. A
/// group has no shape of its own — only its children (separate objects in
/// the same flat list) render anything. A text object currently in inline
/// edit mode also renders nothing here — the editing textarea overlay is
/// its sole visible/interactive surface until the session ends.
fn render_object(object: &DesignObject, onmousedown: Callback<MouseEvent>, ondblclick: Callback<MouseEvent>, is_selected: bool, is_editing: bool) -> Html {
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
        ObjectKind::Rectangle => html! {
            <rect x="0" y="0" width={g.width.to_string()} height={g.height.to_string()} class="canvas-object__rect" />
        },
        ObjectKind::Ellipse => html! {
            <ellipse
                cx={(g.width / 2.0).to_string()}
                cy={(g.height / 2.0).to_string()}
                rx={(g.width / 2.0).to_string()}
                ry={(g.height / 2.0).to_string()}
                class="canvas-object__ellipse"
            />
        },
        ObjectKind::Text(props) => render_text_content(props, g),
        ObjectKind::ImagePlaceholder => html! {
            <>
                <rect x="0" y="0" width={g.width.to_string()} height={g.height.to_string()} class="canvas-object__image-placeholder" />
                <text x={(g.width / 2.0).to_string()} y={(g.height / 2.0).to_string()} dominant-baseline="middle" text-anchor="middle" class="canvas-object__image-label">
                    { kind_icon(&object.kind) }
                </text>
            </>
        },
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
        _ => html! {},
    }
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
        props.fill,
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
    let pointer_action = use_state(|| None::<PointerAction>);
    let guides = use_state(Vec::<Guide>::new);
    let text_area_ref = use_node_ref();
    // The text object's properties/geometry at the moment its edit session
    // started, so `CommitTextEdit` can diff against the (already live)
    // document to build one undo step covering the whole session.
    let editing_snapshot = use_state(|| None::<(ObjectId, TextProperties, Geometry)>);

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

    let on_pointer_down = {
        let editor = editor.clone();
        let svg_ref = svg_ref.clone();
        let pointer_action = pointer_action.clone();
        let guides = guides.clone();
        Callback::from(move |event: MouseEvent| {
            guides.set(Vec::new());
            let Some((x, y)) = canvas_point(&svg_ref, &event) else {
                return;
            };
            match editor.active_tool {
                Tool::Rectangle | Tool::Ellipse => {
                    pointer_action.set(Some(PointerAction::CreateDraft(DragState {
                        start_x: x,
                        start_y: y,
                        current_x: x,
                        current_y: y,
                    })));
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
                Tool::Line | Tool::Pen | Tool::Hand => {}
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
            let Some((x, y)) = canvas_point(&svg_ref, &event) else {
                return;
            };
            match action {
                PointerAction::CreateDraft(state) => {
                    pointer_action.set(Some(PointerAction::CreateDraft(DragState {
                        current_x: x,
                        current_y: y,
                        ..state
                    })));
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
                        compute_snap(&editor.document, &exclude, bbox_of(&candidates), VIEW_WIDTH, VIEW_HEIGHT);
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
                    PointerAction::CreateDraft(state) => {
                        let geometry = state.geometry();
                        if geometry.width >= MIN_DRAG_SIZE && geometry.height >= MIN_DRAG_SIZE {
                            let kind = match editor.active_tool {
                                Tool::Ellipse => ObjectKind::Ellipse,
                                _ => ObjectKind::Rectangle,
                            };
                            editor.dispatch(EditorAction::CreateObject { kind, geometry });
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
                        if let Some((x, y)) = canvas_point(&svg_ref, &event) {
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

    // Converts the SVG's own 800x600 coordinate space to whatever CSS
    // pixel size it's actually rendered at, so the text-edit textarea
    // (a plain HTML element outside the SVG) lines up with it exactly.
    let (overlay_scale_x, overlay_scale_y) = svg_ref
        .cast::<Element>()
        .map(|el| {
            let rect = el.get_bounding_client_rect();
            if rect.width() > 0.0 && rect.height() > 0.0 {
                (rect.width() / VIEW_WIDTH, rect.height() / VIEW_HEIGHT)
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

    html! {
        <section class="canvas-area" tabindex="0">
            <div class="canvas-area__artboard">
                <svg
                    ref={svg_ref.clone()}
                    class="canvas-area__svg"
                    viewBox="0 0 800 600"
                    xmlns="http://www.w3.org/2000/svg"
                    onmousedown={on_pointer_down}
                    onmousemove={on_pointer_move}
                    onmouseup={on_pointer_up}
                >
                    { for editor.document.objects.iter().map(|object| {
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
                            let Some((x, y)) = canvas_point(&svg_ref, &event) else {
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
                        render_object(object, onmousedown, ondblclick, is_selected, is_editing)
                    }) }
                    { match &*pointer_action {
                        Some(PointerAction::CreateDraft(state)) => render_draft(state, editor.active_tool),
                        Some(PointerAction::Marquee(state)) => render_marquee(state),
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
