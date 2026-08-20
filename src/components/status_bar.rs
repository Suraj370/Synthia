use yew::prelude::*;

use crate::editor_state::use_editor;

#[function_component(StatusBar)]
pub fn status_bar() -> Html {
    let editor = use_editor();
    let count = editor.document.objects.len();
    let object_count = format!("{count} object{}", if count == 1 { "" } else { "s" });

    html! {
        <footer class="status-bar">
            <span class="status-bar__item">{"800 × 600"}</span>
            <span class="status-bar__divider"></span>
            <span class="status-bar__item">{"x: 0, y: 0"}</span>
            <span class="status-bar__spacer"></span>
            <span class="status-bar__item">{object_count}</span>
            <span class="status-bar__divider"></span>
            <span class="status-bar__item">{"100%"}</span>
        </footer>
    }
}
