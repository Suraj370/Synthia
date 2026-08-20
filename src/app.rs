use std::collections::BTreeSet;

use web_sys::{Element, KeyboardEvent};
use yew::prelude::*;

use crate::components::canvas_area::CanvasArea;
use crate::components::left_panel::LeftPanel;
use crate::components::right_panel::RightPanel;
use crate::components::status_bar::StatusBar;
use crate::components::top_toolbar::TopToolbar;
use crate::editor_state::{EditorAction, EditorContext, EditorState};
use crate::model::{Geometry, ObjectId};

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
                let tag = target.tag_name();
                if tag == "INPUT" || tag == "TEXTAREA" {
                    return;
                }
            }

            match event.key().as_str() {
                "Escape" => {
                    editor.dispatch(EditorAction::SetSelection(BTreeSet::new()));
                }
                "Delete" => {
                    if !editor.selected_ids.is_empty() {
                        event.prevent_default();
                        editor.dispatch(EditorAction::DeleteObjects(editor.selected_ids.iter().copied().collect()));
                    }
                }
                key @ ("ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown") => {
                    if editor.selected_ids.is_empty() {
                        return;
                    }
                    event.prevent_default();
                    let step = if event.shift_key() { NUDGE_STEP_LARGE } else { NUDGE_STEP };
                    let (dx, dy) = match key {
                        "ArrowLeft" => (-step, 0.0),
                        "ArrowRight" => (step, 0.0),
                        "ArrowUp" => (0.0, -step),
                        "ArrowDown" => (0.0, step),
                        _ => unreachable!(),
                    };
                    let changes: Vec<(ObjectId, Geometry, Geometry)> = editor
                        .selected_ids
                        .iter()
                        .filter_map(|id| {
                            editor.document.get(*id).map(|object| {
                                let before = object.geometry;
                                let after = Geometry { x: before.x + dx, y: before.y + dy, ..before };
                                (*id, before, after)
                            })
                        })
                        .collect();
                    if !changes.is_empty() {
                        editor.dispatch(EditorAction::CommitGeometries(changes));
                    }
                }
                key if (event.ctrl_key() || event.meta_key()) && key.eq_ignore_ascii_case("z") => {
                    event.prevent_default();
                    if event.shift_key() {
                        editor.dispatch(EditorAction::Redo);
                    } else {
                        editor.dispatch(EditorAction::Undo);
                    }
                }
                key if (event.ctrl_key() || event.meta_key()) && key.eq_ignore_ascii_case("d") => {
                    if !editor.selected_ids.is_empty() {
                        event.prevent_default();
                        editor.dispatch(EditorAction::DuplicateObjects(editor.selected_ids.iter().copied().collect()));
                    }
                }
                key if (event.ctrl_key() || event.meta_key()) && key.eq_ignore_ascii_case("c") => {
                    if !editor.selected_ids.is_empty() {
                        event.prevent_default();
                        editor.dispatch(EditorAction::Copy);
                    }
                }
                key if (event.ctrl_key() || event.meta_key()) && key.eq_ignore_ascii_case("v") => {
                    event.prevent_default();
                    editor.dispatch(EditorAction::Paste);
                }
                key if (event.ctrl_key() || event.meta_key()) && key.eq_ignore_ascii_case("g") => {
                    event.prevent_default();
                    if event.shift_key() {
                        editor.dispatch(EditorAction::UngroupSelected);
                    } else {
                        editor.dispatch(EditorAction::GroupSelected);
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
