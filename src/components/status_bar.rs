use yew::prelude::*;

use crate::editor_state::use_editor;

#[function_component(StatusBar)]
pub fn status_bar() -> Html {
    let editor = use_editor();
    let count = editor.document.objects.len();
    let object_count = format!("{count} object{}", if count == 1 { "" } else { "s" });
    let canvas_size = format!("{:.0} × {:.0}", editor.document.canvas_width, editor.document.canvas_height);
    let zoom = format!("{}%", (editor.zoom * 100.0).round() as i64);

    html! {
        <footer class="status-bar">
            <span class="status-bar__item">{canvas_size}</span>
            <span class="status-bar__spacer"></span>
            <span class="status-bar__item">{object_count}</span>
            <span class="status-bar__spacer"></span>
            <span class="status-bar__item status-bar__item--zoom">{zoom}</span>
        </footer>
    }
}
