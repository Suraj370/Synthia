use web_sys::{Element, MouseEvent};
use yew::prelude::*;

use crate::editor_state::{use_editor, EditorAction, Tool};
use crate::model::{DesignObject, Geometry, ObjectKind};

const VIEW_WIDTH: f64 = 800.0;
const VIEW_HEIGHT: f64 = 600.0;
const MIN_DRAG_SIZE: f64 = 2.0;
const TEXT_DEFAULT_WIDTH: f64 = 120.0;
const TEXT_DEFAULT_HEIGHT: f64 = 28.0;

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
fn render_object(object: &DesignObject, onmousedown: Callback<MouseEvent>) -> Html {
    let g = &object.geometry;
    let transform = format!(
        "translate({} {}) rotate({} {} {})",
        g.x,
        g.y,
        g.rotation,
        g.width / 2.0,
        g.height / 2.0
    );

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
        <g key={object.id} {transform} opacity={g.opacity.to_string()} onmousedown={onmousedown} class="canvas-object">
            { inner }
        </g>
    }
}

fn render_selection(object: &DesignObject) -> Html {
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
    let drag = use_state(|| None::<DragState>);

    let on_pointer_down = {
        let editor = editor.clone();
        let svg_ref = svg_ref.clone();
        let drag = drag.clone();
        Callback::from(move |event: MouseEvent| {
            let Some((x, y)) = canvas_point(&svg_ref, &event) else {
                return;
            };
            match editor.active_tool {
                Tool::Rectangle | Tool::Ellipse => {
                    drag.set(Some(DragState {
                        start_x: x,
                        start_y: y,
                        current_x: x,
                        current_y: y,
                    }));
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
        let drag = drag.clone();
        Callback::from(move |event: MouseEvent| {
            let Some(state) = *drag else {
                return;
            };
            let Some((x, y)) = canvas_point(&svg_ref, &event) else {
                return;
            };
            drag.set(Some(DragState {
                current_x: x,
                current_y: y,
                ..state
            }));
        })
    };

    let on_pointer_up = {
        let editor = editor.clone();
        let drag = drag.clone();
        Callback::from(move |_event: MouseEvent| {
            if let Some(state) = *drag {
                let geometry = state.geometry();
                if geometry.width >= MIN_DRAG_SIZE && geometry.height >= MIN_DRAG_SIZE {
                    let kind = match editor.active_tool {
                        Tool::Ellipse => ObjectKind::Ellipse,
                        _ => ObjectKind::Rectangle,
                    };
                    editor.dispatch(EditorAction::CreateObject { kind, geometry });
                }
            }
            drag.set(None);
        })
    };

    let selected = editor.selected_id.and_then(|id| editor.document.get(id));

    html! {
        <section class="canvas-area">
            <div class="canvas-area__artboard">
                <svg
                    ref={svg_ref}
                    class="canvas-area__svg"
                    viewBox="0 0 800 600"
                    xmlns="http://www.w3.org/2000/svg"
                    onmousedown={on_pointer_down}
                    onmousemove={on_pointer_move}
                    onmouseup={on_pointer_up}
                >
                    { for editor.document.objects.iter().map(|object| {
                        let editor = editor.clone();
                        let id = object.id;
                        let onmousedown = Callback::from(move |event: MouseEvent| {
                            if editor.active_tool == Tool::Select {
                                event.stop_propagation();
                                editor.dispatch(EditorAction::SelectObject(Some(id)));
                            }
                        });
                        render_object(object, onmousedown)
                    }) }
                    { if let Some(state) = *drag { render_draft(&state, editor.active_tool) } else { html! {} } }
                    { if let Some(object) = selected { render_selection(object) } else { html! {} } }
                </svg>
            </div>
        </section>
    }
}
