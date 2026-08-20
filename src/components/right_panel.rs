use yew::prelude::*;

use crate::editor_state::{use_editor, EditorAction};
use crate::model::ObjectKind;

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

fn render_field(label: &'static str, value: Option<String>) -> Html {
    html! {
        <label key={label} class="right-panel__field">
            <span class="right-panel__field-label">{label}</span>
            <input
                class="right-panel__field-input"
                type="text"
                value={value.unwrap_or_default()}
                placeholder="—"
                disabled=true
            />
        </label>
    }
}

#[function_component(RightPanel)]
pub fn right_panel() -> Html {
    let editor = use_editor();
    let selected = editor.selected_id.and_then(|id| editor.document.get(id));

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
                                    <li key={object.id} {class} {onclick}>
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
                    { render_field("X", selected.map(|o| format_number(o.geometry.x))) }
                    { render_field("Y", selected.map(|o| format_number(o.geometry.y))) }
                    { render_field("W", selected.map(|o| format_number(o.geometry.width))) }
                    { render_field("H", selected.map(|o| format_number(o.geometry.height))) }
                    { render_field("Rotation", selected.map(|o| format_number(o.geometry.rotation))) }
                    { render_field("Opacity", selected.map(|o| format_number(o.geometry.opacity * 100.0))) }
                </div>
            </div>
        </aside>
    }
}
