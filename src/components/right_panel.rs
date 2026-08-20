use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::editor_state::{use_editor, EditorAction, EditorContext};
use crate::model::{Geometry, ObjectId, ObjectKind, MIN_OBJECT_SIZE};

fn format_number(value: f64) -> String {
    format!("{value:.0}")
}

fn kind_icon(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Rectangle => "▭",
        ObjectKind::Ellipse => "○",
        ObjectKind::Text { .. } => "T",
        ObjectKind::ImagePlaceholder => "▨",
    }
}

fn render_field(label: &'static str, value: Option<f64>, disabled: bool, on_commit: Callback<f64>) -> Html {
    let onchange = Callback::from(move |event: Event| {
        let input: HtmlInputElement = event.target_unchecked_into();
        if let Ok(parsed) = input.value().trim().parse::<f64>() {
            on_commit.emit(parsed);
        }
    });

    html! {
        <label key={label} class="right-panel__field">
            <span class="right-panel__field-label">{label}</span>
            <input
                class="right-panel__field-input"
                type="text"
                value={value.map(format_number).unwrap_or_default()}
                placeholder="—"
                {disabled}
                {onchange}
            />
        </label>
    }
}

/// Builds a Properties field bound to one `Geometry` component: `get` reads
/// the display value, `set` writes a committed edit back onto a copy of the
/// selected object's geometry before it's dispatched.
fn geometry_field(
    editor: &EditorContext,
    selected: Option<(ObjectId, Geometry)>,
    label: &'static str,
    get: fn(&Geometry) -> f64,
    set: fn(&mut Geometry, f64),
) -> Html {
    let value = selected.map(|(_, g)| get(&g));
    let editor = editor.clone();
    let on_commit = Callback::from(move |input: f64| {
        if let Some((id, mut geometry)) = selected {
            set(&mut geometry, input);
            editor.dispatch(EditorAction::UpdateGeometry { id, geometry });
        }
    });
    render_field(label, value, selected.is_none(), on_commit)
}

#[function_component(RightPanel)]
pub fn right_panel() -> Html {
    let editor = use_editor();
    let selected = editor
        .selected_id
        .and_then(|id| editor.document.get(id).map(|object| (id, object.geometry)));

    html! {
        <aside class="right-panel">
            <div class="right-panel__section right-panel__section--layers">
                <h2 class="right-panel__heading">{"Layers"}</h2>
                { if editor.document.objects.is_empty() {
                    html! { <div class="right-panel__empty">{"No layers yet"}</div> }
                } else {
                    html! {
                        <ul class="right-panel__layers">
                            { for editor.document.objects.iter().rev().map(|object| {
                                let is_selected = editor.selected_id == Some(object.id);
                                let class = if is_selected {
                                    "right-panel__layer right-panel__layer--active"
                                } else {
                                    "right-panel__layer"
                                };
                                let editor = editor.clone();
                                let id = object.id;
                                let onclick = Callback::from(move |_| editor.dispatch(EditorAction::SelectObject(Some(id))));
                                html! {
                                    <li key={object.id} {class} tabindex="0" {onclick}>
                                        <span class="right-panel__layer-icon">{kind_icon(&object.kind)}</span>
                                        <span class="right-panel__layer-name">{object.name.clone()}</span>
                                    </li>
                                }
                            }) }
                        </ul>
                    }
                } }
            </div>

            <div class="right-panel__section right-panel__section--properties">
                <h2 class="right-panel__heading">{"Properties"}</h2>
                <div class="right-panel__fields">
                    { geometry_field(&editor, selected, "X", |g| g.x, |g, v| g.x = v) }
                    { geometry_field(&editor, selected, "Y", |g| g.y, |g, v| g.y = v) }
                    { geometry_field(&editor, selected, "W", |g| g.width, |g, v| g.width = v.max(MIN_OBJECT_SIZE)) }
                    { geometry_field(&editor, selected, "H", |g| g.height, |g, v| g.height = v.max(MIN_OBJECT_SIZE)) }
                    { geometry_field(&editor, selected, "Rotation", |g| g.rotation, |g, v| g.rotation = v) }
                    { geometry_field(&editor, selected, "Opacity", |g| g.opacity * 100.0, |g, v| g.opacity = (v / 100.0).clamp(0.0, 1.0)) }
                </div>
            </div>
        </aside>
    }
}
