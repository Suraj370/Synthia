use web_sys::{Element, KeyboardEvent};
use yew::prelude::*;

use crate::components::canvas_area::CanvasArea;
use crate::components::left_panel::LeftPanel;
use crate::components::right_panel::RightPanel;
use crate::components::status_bar::StatusBar;
use crate::components::top_toolbar::TopToolbar;
use crate::editor_state::{EditorAction, EditorContext, EditorState};

const NUDGE_STEP: f64 = 1.0;
const NUDGE_STEP_LARGE: f64 = 10.0;

#[function_component(App)]
pub fn app() -> Html {
    let editor = use_reducer(EditorState::default);

    let on_key_down = {
        let editor = editor.clone();
        Callback::from(move |event: KeyboardEvent| {
            // Let typing in a Properties field behave normally instead of
            // being hijacked as a canvas shortcut.
            if let Some(target) = event.target_dyn_into::<Element>() {
                if target.tag_name() == "INPUT" {
                    return;
                }
            }

            match event.key().as_str() {
                "Escape" => {
                    editor.dispatch(EditorAction::SelectObject(None));
                }
                "Delete" => {
                    if let Some(id) = editor.selected_id {
                        event.prevent_default();
                        editor.dispatch(EditorAction::DeleteObject(id));
                    }
                }
                key @ ("ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown") => {
                    let Some(id) = editor.selected_id else { return };
                    let Some(object) = editor.document.get(id) else { return };
                    event.prevent_default();
                    let step = if event.shift_key() { NUDGE_STEP_LARGE } else { NUDGE_STEP };
                    let mut geometry = object.geometry;
                    match key {
                        "ArrowLeft" => geometry.x -= step,
                        "ArrowRight" => geometry.x += step,
                        "ArrowUp" => geometry.y -= step,
                        "ArrowDown" => geometry.y += step,
                        _ => unreachable!(),
                    }
                    editor.dispatch(EditorAction::UpdateGeometry { id, geometry });
                }
                key if (event.ctrl_key() || event.meta_key()) && key.eq_ignore_ascii_case("d") => {
                    if let Some(id) = editor.selected_id {
                        event.prevent_default();
                        editor.dispatch(EditorAction::DuplicateObject(id));
                    }
                }
                _ => {}
            }
        })
    };

    html! {
        <ContextProvider<EditorContext> context={editor}>
            <div class="app-shell" onkeydown={on_key_down}>
                <TopToolbar />
                <div class="app-shell__body">
                    <LeftPanel />
                    <CanvasArea />
                    <RightPanel />
                </div>
                <StatusBar />
            </div>
        </ContextProvider<EditorContext>>
    }
}
