use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::editor_state::{use_editor, EditorAction};
use crate::image_import::{files_from_input, import_image_file, ImportTarget};

/// Matches `canvas_area`'s own artboard size — there's no drop point to
/// place an Open-menu import at, so it centers on the artboard instead.
const CANVAS_CENTER: (f64, f64) = (400.0, 300.0);

#[function_component(TopToolbar)]
pub fn top_toolbar() -> Html {
    let editor = use_editor();
    let open_input_ref = use_node_ref();

    let on_undo = {
        let editor = editor.clone();
        Callback::from(move |_| editor.dispatch(EditorAction::Undo))
    };
    let on_redo = {
        let editor = editor.clone();
        Callback::from(move |_| editor.dispatch(EditorAction::Redo))
    };

    // "Open" doubles as Apollo's image-import entry point — there's no
    // project file format to open yet (Save is still a placeholder too),
    // so this is the toolbar's one existing menu action that makes sense
    // to wire up for it.
    let on_open_click = {
        let open_input_ref = open_input_ref.clone();
        Callback::from(move |_| {
            if let Some(input) = open_input_ref.cast::<HtmlInputElement>() {
                input.click();
            }
        })
    };
    let on_open_change = {
        let editor = editor.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            for file in files_from_input(&input) {
                import_image_file(editor.clone(), file, ImportTarget::NewObject { center: CANVAS_CENTER });
            }
            input.set_value("");
        })
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
                <button class="toolbar__button" title="Open image" onclick={on_open_click}>{"Open"}</button>
                <input
                    ref={open_input_ref}
                    type="file"
                    accept="image/png,image/jpeg,image/webp"
                    multiple=true
                    class="hidden-file-input"
                    onchange={on_open_change}
                />
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
