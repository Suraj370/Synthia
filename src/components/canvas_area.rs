use web_sys::{Element, MouseEvent};
use yew::prelude::*;

use crate::editor_state::{use_editor, EditorAction, Tool};
use crate::model::{DesignObject, Geometry, ObjectId, ObjectKind, MIN_OBJECT_SIZE};

const VIEW_WIDTH: f64 = 800.0;
const VIEW_HEIGHT: f64 = 600.0;
const MIN_DRAG_SIZE: f64 = 2.0;
const TEXT_DEFAULT_WIDTH: f64 = 120.0;
const TEXT_DEFAULT_HEIGHT: f64 = 28.0;
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

/// What the mouse is currently doing on the canvas. `origin` fields hold the
/// object's geometry as it was when the drag started, so each mousemove
/// recomputes the new geometry from that fixed baseline instead of
/// compounding small deltas.
#[derive(Clone, Copy, PartialEq)]
enum PointerAction {
    CreateDraft(DragState),
    MoveObject { id: ObjectId, origin: Geometry, start_x: f64, start_y: f64 },
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
        ObjectKind::Text { .. } => "T",
        ObjectKind::ImagePlaceholder => "▨",
    }
}

/// Renders one object's shape, positioned via a translate+rotate transform
/// so every kind shares the same rotation/opacity/hit-testing wiring.
fn render_object(object: &DesignObject, onmousedown: Callback<MouseEvent>, is_selected: bool) -> Html {
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
        ObjectKind::Text { content } => html! {
            <>
                <rect x="0" y="0" width={g.width.to_string()} height={g.height.to_string()} fill="transparent" />
                <text x="6" y={(g.height / 2.0).to_string()} dominant-baseline="middle" class="canvas-object__text">
                    { content.clone() }
                </text>
            </>
        },
        ObjectKind::ImagePlaceholder => html! {
            <>
                <rect x="0" y="0" width={g.width.to_string()} height={g.height.to_string()} class="canvas-object__image-placeholder" />
                <text x={(g.width / 2.0).to_string()} y={(g.height / 2.0).to_string()} dominant-baseline="middle" text-anchor="middle" class="canvas-object__image-label">
                    { kind_icon(&object.kind) }
                </text>
            </>
        },
    };

    html! {
        <g key={object.id} {transform} opacity={g.opacity.to_string()} onmousedown={onmousedown} {class}>
            { inner }
        </g>
    }
}

fn render_selection(object: &DesignObject, on_handle_down: Callback<(HandleKind, MouseEvent)>) -> Html {
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
                    on_handle_down.emit((handle, event));
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

#[function_component(CanvasArea)]
pub fn canvas_area() -> Html {
    let editor = use_editor();
    let svg_ref = use_node_ref();
    let pointer_action = use_state(|| None::<PointerAction>);

    let on_pointer_down = {
        let editor = editor.clone();
        let svg_ref = svg_ref.clone();
        let pointer_action = pointer_action.clone();
        Callback::from(move |event: MouseEvent| {
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
                    editor.dispatch(EditorAction::CreateObject {
                        kind: ObjectKind::Text { content: "Text".to_string() },
                        geometry: Geometry::new(x, y, TEXT_DEFAULT_WIDTH, TEXT_DEFAULT_HEIGHT),
                    });
                }
                Tool::Select => {
                    editor.dispatch(EditorAction::SelectObject(None));
                }
                Tool::Line | Tool::Pen | Tool::Hand => {}
            }
        })
    };

    let on_pointer_move = {
        let svg_ref = svg_ref.clone();
        let pointer_action = pointer_action.clone();
        let editor = editor.clone();
        Callback::from(move |event: MouseEvent| {
            let Some(action) = *pointer_action else {
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
                PointerAction::MoveObject { id, origin, start_x, start_y } => {
                    let geometry = Geometry {
                        x: origin.x + (x - start_x),
                        y: origin.y + (y - start_y),
                        ..origin
                    };
                    editor.dispatch(EditorAction::UpdateGeometry { id, geometry });
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
        Callback::from(move |_event: MouseEvent| {
            if let Some(PointerAction::CreateDraft(state)) = *pointer_action {
                let geometry = state.geometry();
                if geometry.width >= MIN_DRAG_SIZE && geometry.height >= MIN_DRAG_SIZE {
                    let kind = match editor.active_tool {
                        Tool::Ellipse => ObjectKind::Ellipse,
                        _ => ObjectKind::Rectangle,
                    };
                    editor.dispatch(EditorAction::CreateObject { kind, geometry });
                }
            }
            pointer_action.set(None);
        })
    };

    let on_handle_down = {
        let editor = editor.clone();
        let pointer_action = pointer_action.clone();
        Callback::from(move |(handle, _event): (HandleKind, MouseEvent)| {
            if editor.active_tool != Tool::Select {
                return;
            }
            let Some(id) = editor.selected_id else {
                return;
            };
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

    let selected = editor.selected_id.and_then(|id| editor.document.get(id));

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
                        let is_selected = editor.selected_id == Some(id);
                        let onmousedown = Callback::from(move |event: MouseEvent| {
                            if editor.active_tool != Tool::Select {
                                return;
                            }
                            event.stop_propagation();
                            editor.dispatch(EditorAction::SelectObject(Some(id)));
                            if let (Some((x, y)), Some(object)) = (canvas_point(&svg_ref, &event), editor.document.get(id)) {
                                pointer_action.set(Some(PointerAction::MoveObject {
                                    id,
                                    origin: object.geometry,
                                    start_x: x,
                                    start_y: y,
                                }));
                            }
                        });
                        render_object(object, onmousedown, is_selected)
                    }) }
                    { match *pointer_action {
                        Some(PointerAction::CreateDraft(state)) => render_draft(&state, editor.active_tool),
                        _ => html! {},
                    } }
                    { if let Some(object) = selected { render_selection(object, on_handle_down) } else { html! {} } }
                </svg>
            </div>
        </section>
    }
}
