use yew::prelude::*;

use crate::editor_state::{use_editor, EditorAction};

#[function_component(TopToolbar)]
pub fn top_toolbar() -> Html {
    let editor = use_editor();

    let on_undo = {
        let editor = editor.clone();
        Callback::from(move |_| editor.dispatch(EditorAction::Undo))
    };
    let on_redo = {
        let editor = editor.clone();
        Callback::from(move |_| editor.dispatch(EditorAction::Redo))
    };

    html! {
        <header class="toolbar">
            <div class="toolbar__section toolbar__section--brand">
                <span class="toolbar__logo">{"◆"}</span>
                <span class="toolbar__title">{"Apollo"}</span>
            </div>

            <div class="toolbar__divider"></div>

            <div class="toolbar__section toolbar__section--actions">
                <button class="toolbar__button" title="New document">{"New"}</button>
                <button class="toolbar__button" title="Open document">{"Open"}</button>
                <button class="toolbar__button" title="Save document">{"Save"}</button>
            </div>

            <div class="toolbar__section toolbar__section--doc">
                <span class="toolbar__doc-name">{"Untitled"}</span>
            </div>

            <div class="toolbar__section toolbar__section--history">
                <button
                    class="toolbar__icon-button"
                    title="Undo (Ctrl+Z)"
                    disabled={!editor.history.can_undo()}
                    onclick={on_undo}
                >
                    {"↶"}
                </button>
                <button
                    class="toolbar__icon-button"
                    title="Redo (Ctrl+Shift+Z)"
                    disabled={!editor.history.can_redo()}
                    onclick={on_redo}
                >
                    {"↷"}
                </button>
            </div>

            <div class="toolbar__divider"></div>

            <div class="toolbar__section toolbar__section--view">
                <button class="toolbar__icon-button" title="Zoom out">{"−"}</button>
                <span class="toolbar__zoom">{"100%"}</span>
                <button class="toolbar__icon-button" title="Zoom in">{"+"}</button>
            </div>

            <div class="toolbar__divider"></div>

            <button class="toolbar__button toolbar__button--primary" title="Export document">{"Export"}</button>
        </header>
    }
}
