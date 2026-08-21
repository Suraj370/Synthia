use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::components::document_settings::DocumentSettingsPanel;
use crate::document_io::{self, OpenOutcome, SaveOutcome};
use crate::editor_state::{use_editor, EditorAction, EditorContext, MAX_ZOOM, MIN_ZOOM};
use crate::export::{self, ExportOutcome};
use crate::image_import::{files_from_input, import_image_file, ImportTarget};

const ZOOM_STEP: f64 = 0.25;
const ZOOM_PRESETS: [f64; 7] = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0];

fn format_zoom(zoom: f64) -> String {
    format!("{}%", (zoom * 100.0).round() as i64)
}

/// Approximates how much screen space the artboard actually has to work
/// with, from the window size minus the surrounding chrome's known fixed
/// sizes (toolbar/status bar heights, left tool rail, right panel). Good
/// enough for "fit to screen/selection" — a convenience nudge, not a
/// pixel-exact fit — without plumbing a canvas container ref through to
/// the toolbar for it.
fn approximate_viewport() -> (f64, f64) {
    const CHROME_WIDTH: f64 = 52.0 + 320.0 + 80.0;
    const CHROME_HEIGHT: f64 = 44.0 + 26.0 + 80.0;
    let window = web_sys::window();
    let width = window.as_ref().and_then(|w| w.inner_width().ok()).and_then(|v| v.as_f64()).unwrap_or(1280.0);
    let height = window.as_ref().and_then(|w| w.inner_height().ok()).and_then(|v| v.as_f64()).unwrap_or(800.0);
    ((width - CHROME_WIDTH).max(200.0), (height - CHROME_HEIGHT).max(150.0))
}

fn compute_fit_to_selection(editor: &EditorContext) -> Option<f64> {
    if editor.selected_ids.is_empty() {
        return None;
    }
    let ids: Vec<_> = editor.selected_ids.iter().copied().collect();
    let bbox = editor.document.bounding_box(&ids)?;
    if bbox.width <= 0.0 || bbox.height <= 0.0 {
        return None;
    }
    let (available_width, available_height) = approximate_viewport();
    let zoom = (available_width / bbox.width).min(available_height / bbox.height);
    Some(zoom.clamp(MIN_ZOOM, MAX_ZOOM))
}

/// Runs the shared Save flow: writes to `existing_path` if given, else
/// shows the native save dialog. Used by both the Save button (passes the
/// document's current path, if any) and Save As (always passes `None`).
/// `on_saved` fires only on a real save — cancelling the dialog or a
/// failure both leave the document untouched.
fn spawn_save(editor: EditorContext, existing_path: Option<String>, error: UseStateHandle<Option<String>>, on_saved: Callback<()>) {
    let suggested_name = Some(editor.document_title.clone());
    let document = editor.document.clone();
    wasm_bindgen_futures::spawn_local(async move {
        match document_io::save(&document, existing_path, suggested_name).await {
            Ok(SaveOutcome::Saved { path, filename }) => {
                editor.dispatch(EditorAction::MarkSaved { path, filename });
                on_saved.emit(());
            }
            Ok(SaveOutcome::Cancelled) => {}
            Err(message) => error.set(Some(message)),
        }
    });
}

fn spawn_open(editor: EditorContext, error: UseStateHandle<Option<String>>) {
    wasm_bindgen_futures::spawn_local(async move {
        match document_io::open().await {
            Ok(OpenOutcome::Opened { document, path, filename }) => {
                editor.dispatch(EditorAction::LoadDocument { document, path, filename });
            }
            Ok(OpenOutcome::Cancelled) => {}
            Err(message) => error.set(Some(message)),
        }
    });
}

/// Runs the Export flow for one format: builds the file (SVG serialization
/// is instant; PNG additionally rasterizes via an offscreen canvas — see
/// `export.rs`) and hands it to the native save dialog. Mirrors
/// `spawn_save`'s shape; kept separate since Export always prompts for a
/// location (no "existing path" concept) and has two formats instead of one.
fn spawn_export(editor: EditorContext, as_svg: bool, error: UseStateHandle<Option<String>>) {
    let suggested_name = editor.document_title.clone();
    let document = editor.document.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let result = if as_svg { export::export_svg(&document, suggested_name).await } else { export::export_png(&document, suggested_name).await };
        match result {
            Ok(ExportOutcome::Exported) | Ok(ExportOutcome::Cancelled) => {}
            Err(message) => error.set(Some(message)),
        }
    });
}

#[function_component(TopToolbar)]
pub fn top_toolbar() -> Html {
    let editor = use_editor();
    let import_input_ref = use_node_ref();
    let show_unsaved_dialog = use_state(|| false);
    let show_zoom_menu = use_state(|| false);
    let show_export_menu = use_state(|| false);
    let show_canvas_settings = use_state(|| false);
    let error_message = use_state(|| None::<String>);

    let on_undo = {
        let editor = editor.clone();
        Callback::from(move |_| editor.dispatch(EditorAction::Undo))
    };
    let on_redo = {
        let editor = editor.clone();
        Callback::from(move |_| editor.dispatch(EditorAction::Redo))
    };

    // Image import now lives on its own button — Open is Apollo's own
    // document format, per the file-system fix this toolbar is part of.
    let on_import_click = {
        let import_input_ref = import_input_ref.clone();
        Callback::from(move |_| {
            if let Some(input) = import_input_ref.cast::<HtmlInputElement>() {
                input.click();
            }
        })
    };
    let on_import_change = {
        let editor = editor.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            // There's no drop point to place an imported image at from
            // this button — it centers on the current artboard instead
            // (drag-and-drop onto the canvas remains the position-aware
            // way to import).
            let center = (editor.document.canvas_width / 2.0, editor.document.canvas_height / 2.0);
            for file in files_from_input(&input) {
                import_image_file(editor.clone(), file, ImportTarget::NewObject { center });
            }
            input.set_value("");
        })
    };

    let on_new_click = {
        let editor = editor.clone();
        let show_unsaved_dialog = show_unsaved_dialog.clone();
        Callback::from(move |_| {
            if editor.is_dirty {
                show_unsaved_dialog.set(true);
            } else {
                editor.dispatch(EditorAction::NewDocument);
            }
        })
    };
    let on_dialog_cancel = {
        let show_unsaved_dialog = show_unsaved_dialog.clone();
        Callback::from(move |_| show_unsaved_dialog.set(false))
    };
    let on_dialog_dont_save = {
        let editor = editor.clone();
        let show_unsaved_dialog = show_unsaved_dialog.clone();
        Callback::from(move |_| {
            editor.dispatch(EditorAction::NewDocument);
            show_unsaved_dialog.set(false);
        })
    };
    let on_dialog_save = {
        let editor = editor.clone();
        let show_unsaved_dialog = show_unsaved_dialog.clone();
        let error_message = error_message.clone();
        Callback::from(move |_| {
            let show_unsaved_dialog = show_unsaved_dialog.clone();
            let on_saved = {
                let editor = editor.clone();
                let show_unsaved_dialog = show_unsaved_dialog.clone();
                Callback::from(move |()| {
                    editor.dispatch(EditorAction::NewDocument);
                    show_unsaved_dialog.set(false);
                })
            };
            spawn_save(editor.clone(), editor.file_path.clone(), error_message.clone(), on_saved);
        })
    };

    let on_save_click = {
        let editor = editor.clone();
        let error_message = error_message.clone();
        Callback::from(move |_| {
            spawn_save(editor.clone(), editor.file_path.clone(), error_message.clone(), Callback::noop());
        })
    };
    let on_save_as_click = {
        let editor = editor.clone();
        let error_message = error_message.clone();
        Callback::from(move |_| {
            spawn_save(editor.clone(), None, error_message.clone(), Callback::noop());
        })
    };
    let on_open_click = {
        let editor = editor.clone();
        let error_message = error_message.clone();
        Callback::from(move |_| spawn_open(editor.clone(), error_message.clone()))
    };
    let on_dismiss_error = {
        let error_message = error_message.clone();
        Callback::from(move |_| error_message.set(None))
    };

    let on_zoom_out = {
        let editor = editor.clone();
        Callback::from(move |_| editor.dispatch(EditorAction::SetZoom(editor.zoom - ZOOM_STEP)))
    };
    let on_zoom_in = {
        let editor = editor.clone();
        Callback::from(move |_| editor.dispatch(EditorAction::SetZoom(editor.zoom + ZOOM_STEP)))
    };
    let on_zoom_label_click = {
        let show_zoom_menu = show_zoom_menu.clone();
        Callback::from(move |_| show_zoom_menu.set(!*show_zoom_menu))
    };
    let on_fit_click = {
        let editor = editor.clone();
        Callback::from(move |_| editor.dispatch(EditorAction::SetFitMode(true)))
    };

    let on_export_click = {
        let show_export_menu = show_export_menu.clone();
        Callback::from(move |_| show_export_menu.set(!*show_export_menu))
    };
    let on_export_svg = {
        let editor = editor.clone();
        let error_message = error_message.clone();
        let show_export_menu = show_export_menu.clone();
        Callback::from(move |_| {
            spawn_export(editor.clone(), true, error_message.clone());
            show_export_menu.set(false);
        })
    };
    let on_export_png = {
        let editor = editor.clone();
        let error_message = error_message.clone();
        let show_export_menu = show_export_menu.clone();
        Callback::from(move |_| {
            spawn_export(editor.clone(), false, error_message.clone());
            show_export_menu.set(false);
        })
    };

    let on_canvas_settings_click = {
        let show_canvas_settings = show_canvas_settings.clone();
        Callback::from(move |_| show_canvas_settings.set(!*show_canvas_settings))
    };
    let on_canvas_settings_close = {
        let show_canvas_settings = show_canvas_settings.clone();
        Callback::from(move |()| show_canvas_settings.set(false))
    };

    let doc_title = if editor.is_dirty { format!("{} •", editor.document_title) } else { editor.document_title.clone() };

    html! {
        <>
            <header class="toolbar">
                <div class="toolbar__section toolbar__section--brand">
                    <span class="toolbar__logo">{"◆"}</span>
                    <span class="toolbar__title">{"Apollo"}</span>
                </div>

                <div class="toolbar__divider"></div>

                <div class="toolbar__section toolbar__section--actions">
                    <button class="toolbar__button" title="New document" onclick={on_new_click}>{"New"}</button>
                    <button class="toolbar__button" title="Open document (.apollo)" onclick={on_open_click}>{"Open"}</button>
                    <button class="toolbar__button" title="Save document" onclick={on_save_click}>{"Save"}</button>
                    <button class="toolbar__button" title="Save document as…" onclick={on_save_as_click}>{"Save As"}</button>
                    <button class="toolbar__button" title="Import image" onclick={on_import_click}>{"Import"}</button>
                    <input
                        ref={import_input_ref}
                        type="file"
                        accept="image/png,image/jpeg,image/webp"
                        multiple=true
                        class="hidden-file-input"
                        onchange={on_import_change}
                    />
                </div>

                <div class="toolbar__section toolbar__section--doc">
                    <span class="toolbar__doc-name">{doc_title}</span>
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
                    <button class="toolbar__icon-button" title="Zoom out" onclick={on_zoom_out}>{"−"}</button>
                    <div class="toolbar__zoom-control">
                        <button class="toolbar__zoom" onclick={on_zoom_label_click}>{ format_zoom(editor.zoom) }</button>
                        { if *show_zoom_menu {
                            render_zoom_menu(&editor, show_zoom_menu.clone())
                        } else {
                            html! {}
                        } }
                    </div>
                    <button class="toolbar__icon-button" title="Zoom in" onclick={on_zoom_in}>{"+"}</button>
                    <button
                        class={classes!("toolbar__button", editor.fit_mode.then_some("toolbar__button--primary"))}
                        title="Fit canvas to workspace"
                        onclick={on_fit_click}
                    >
                        {"Fit"}
                    </button>
                </div>

                <div class="toolbar__divider"></div>

                <div class="toolbar__zoom-control">
                    <button class="toolbar__button" title="Canvas size and background" onclick={on_canvas_settings_click}>{"Canvas"}</button>
                    { if *show_canvas_settings {
                        html! { <DocumentSettingsPanel on_close={on_canvas_settings_close} /> }
                    } else {
                        html! {}
                    } }
                </div>

                <div class="toolbar__divider"></div>

                <div class="toolbar__zoom-control">
                    <button class="toolbar__button toolbar__button--primary" title="Export document" onclick={on_export_click}>{"Export"}</button>
                    { if *show_export_menu {
                        html! {
                            <div class="zoom-menu zoom-menu--right-align">
                                <button type="button" class="zoom-menu__item" onclick={on_export_svg}>{"Export as SVG"}</button>
                                <button type="button" class="zoom-menu__item" onclick={on_export_png}>{"Export as PNG"}</button>
                            </div>
                        }
                    } else {
                        html! {}
                    } }
                </div>
            </header>

            { if *show_unsaved_dialog {
                render_unsaved_dialog(on_dialog_cancel, on_dialog_dont_save, on_dialog_save)
            } else {
                html! {}
            } }

            { if let Some(message) = (*error_message).clone() {
                html! {
                    <div class="error-banner">
                        <span class="error-banner__message">{ message }</span>
                        <button type="button" class="error-banner__dismiss" onclick={on_dismiss_error}>{"✕"}</button>
                    </div>
                }
            } else {
                html! {}
            } }
        </>
    }
}

fn render_unsaved_dialog(on_cancel: Callback<MouseEvent>, on_dont_save: Callback<MouseEvent>, on_save: Callback<MouseEvent>) -> Html {
    html! {
        <div class="modal-overlay">
            <div class="modal-dialog">
                <p class="modal-dialog__message">{"You have unsaved changes. Create a new document anyway?"}</p>
                <div class="modal-dialog__actions">
                    <button type="button" class="toolbar__button" onclick={on_cancel}>{"Cancel"}</button>
                    <button type="button" class="toolbar__button" onclick={on_dont_save}>{"Don't Save"}</button>
                    <button type="button" class="toolbar__button toolbar__button--primary" onclick={on_save}>{"Save"}</button>
                </div>
            </div>
        </div>
    }
}

fn render_zoom_menu(editor: &EditorContext, show_zoom_menu: UseStateHandle<bool>) -> Html {
    let preset_button = |zoom: f64| {
        let editor = editor.clone();
        let show_zoom_menu = show_zoom_menu.clone();
        let onclick = Callback::from(move |_| {
            editor.dispatch(EditorAction::SetZoom(zoom));
            show_zoom_menu.set(false);
        });
        html! { <button type="button" class="zoom-menu__item" {onclick}>{ format_zoom(zoom) }</button> }
    };

    let on_fit_screen = {
        let editor = editor.clone();
        let show_zoom_menu = show_zoom_menu.clone();
        Callback::from(move |_| {
            editor.dispatch(EditorAction::SetFitMode(true));
            show_zoom_menu.set(false);
        })
    };
    let fit_selection_zoom = compute_fit_to_selection(editor);
    let on_fit_selection = {
        let editor = editor.clone();
        let show_zoom_menu = show_zoom_menu.clone();
        Callback::from(move |_| {
            if let Some(zoom) = fit_selection_zoom {
                editor.dispatch(EditorAction::SetZoom(zoom));
                show_zoom_menu.set(false);
            }
        })
    };

    html! {
        <div class="zoom-menu">
            { for ZOOM_PRESETS.iter().map(|&zoom| preset_button(zoom)) }
            <div class="zoom-menu__divider"></div>
            <button type="button" class="zoom-menu__item" onclick={on_fit_screen}>{"Fit to screen"}</button>
            <button type="button" class="zoom-menu__item" disabled={fit_selection_zoom.is_none()} onclick={on_fit_selection}>{"Fit to selection"}</button>
        </div>
    }
}
