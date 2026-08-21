//! The Canvas Size popover: direct control over the document's actual
//! width/height and background — as opposed to zoom (`toolbar__zoom-control`
//! next to it), which only changes how large the artboard appears on
//! screen and never touches `DesignDocument` at all. Every field here
//! writes straight to `DesignDocument::canvas_width`/`canvas_height`/
//! `background` via `EditorAction`, so it's undoable and saves/exports
//! like any other document change.

use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::color::parse_hex;
use crate::components::color_picker::ColorField;
use crate::editor_state::{use_editor, EditorAction, EditorContext};
use crate::model::{Color, MIN_CANVAS_SIZE};

struct Preset {
    label: &'static str,
    width: f64,
    height: f64,
}

const fn preset(label: &'static str, width: f64, height: f64) -> Preset {
    Preset { label, width, height }
}

const SOCIAL_PRESETS: &[Preset] = &[
    preset("Instagram Post", 1080.0, 1080.0),
    preset("Instagram Story", 1080.0, 1920.0),
    preset("YouTube Thumbnail", 1280.0, 720.0),
    preset("YouTube Banner", 2560.0, 1440.0),
];

const PRINT_PRESETS: &[Preset] = &[preset("A4 Portrait", 2480.0, 3508.0), preset("A4 Landscape", 3508.0, 2480.0)];

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a.max(1)
    } else {
        gcd(b, a % b)
    }
}

/// The document's current width/height reduced to a small integer ratio,
/// e.g. "16:9" — a read-only display, not an independently editable field
/// (editing width/height already fully determines it).
fn format_aspect_ratio(width: f64, height: f64) -> String {
    let w = (width.round().max(1.0)) as u64;
    let h = (height.round().max(1.0)) as u64;
    let divisor = gcd(w, h);
    format!("{}:{}", w / divisor, h / divisor)
}

/// Whether `width`/`height` exactly match one of the predefined presets —
/// used only for the read-only "Preset" label, never to keep a preset
/// button looking selected (presets are one-shot actions, not a persistent
/// mode: see this file's module docs).
fn matched_preset_label(width: f64, height: f64) -> &'static str {
    SOCIAL_PRESETS
        .iter()
        .chain(PRINT_PRESETS)
        .find(|item| (width - item.width).abs() < 0.5 && (height - item.height).abs() < 0.5)
        .map(|item| item.label)
        .unwrap_or("Custom")
}

/// One preset button: applies its exact width/height (bypassing the aspect
/// lock, which only governs typing into the Width/Height fields) with
/// whatever "Scale content" is currently set to. Deliberately stateless —
/// it never renders as "selected," even if the document's current size
/// happens to match it, since a preset is a one-shot action (apply these
/// numbers) rather than a persistent mode the document remembers.
fn render_preset_button(editor: &EditorContext, scale_content: bool, item: &'static Preset) -> Html {
    let editor = editor.clone();
    let onclick = Callback::from(move |_: MouseEvent| {
        editor.dispatch(EditorAction::SetCanvasSize { width: item.width, height: item.height, scale_content });
    });
    html! {
        <button type="button" key={item.label} class="canvas-settings__preset" {onclick}>
            <span class="canvas-settings__preset-label">{item.label}</span>
            <span class="canvas-settings__preset-dims">{ format!("{:.0} × {:.0}", item.width, item.height) }</span>
        </button>
    }
}

#[derive(Properties, PartialEq)]
pub struct DocumentSettingsPanelProps {
    pub on_close: Callback<()>,
}

#[function_component(DocumentSettingsPanel)]
pub fn document_settings_panel(props: &DocumentSettingsPanelProps) -> Html {
    let editor = use_editor();
    // Whether editing one of width/height should proportionally update the
    // other. Purely a UI preference for how the fields below behave — not
    // document data, so it isn't undoable and doesn't persist.
    let aspect_locked = use_state(|| false);
    // Whether the next width/height/preset change should also scale every
    // object's geometry along with the canvas. Same "UI-only toggle" shape
    // as `aspect_locked`; see `EditorAction::SetCanvasSize`.
    let scale_content = use_state(|| false);

    let canvas_width = editor.document.canvas_width;
    let canvas_height = editor.document.canvas_height;

    let on_width_change = {
        let editor = editor.clone();
        let aspect_locked = *aspect_locked;
        let scale_content = *scale_content;
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            if let Ok(width) = input.value().trim().parse::<f64>() {
                if width > 0.0 {
                    let height = if aspect_locked && canvas_width > 0.0 {
                        (canvas_height * (width / canvas_width)).max(MIN_CANVAS_SIZE)
                    } else {
                        canvas_height
                    };
                    editor.dispatch(EditorAction::SetCanvasSize { width, height, scale_content });
                }
            }
        })
    };

    let on_height_change = {
        let editor = editor.clone();
        let aspect_locked = *aspect_locked;
        let scale_content = *scale_content;
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            if let Ok(height) = input.value().trim().parse::<f64>() {
                if height > 0.0 {
                    let width = if aspect_locked && canvas_height > 0.0 {
                        (canvas_width * (height / canvas_height)).max(MIN_CANVAS_SIZE)
                    } else {
                        canvas_width
                    };
                    editor.dispatch(EditorAction::SetCanvasSize { width, height, scale_content });
                }
            }
        })
    };

    let on_lock_toggle = {
        let aspect_locked = aspect_locked.clone();
        Callback::from(move |_: MouseEvent| aspect_locked.set(!*aspect_locked))
    };

    let on_scale_content_toggle = {
        let scale_content = scale_content.clone();
        Callback::from(move |_: Event| scale_content.set(!*scale_content))
    };

    let background_color = parse_hex(&editor.document.background).map(|(r, g, b)| Color::rgb(r, g, b)).unwrap_or(Color::rgb(0x19, 0x19, 0x1c));
    let on_background_live = {
        let editor = editor.clone();
        Callback::from(move |color: Color| editor.dispatch(EditorAction::UpdateBackground(color.to_hex())))
    };
    let on_background_commit = {
        let editor = editor.clone();
        Callback::from(move |(before, after): (Color, Color)| {
            editor.dispatch(EditorAction::CommitBackground { before: before.to_hex(), after: after.to_hex() });
        })
    };

    let on_reset_defaults = {
        let editor = editor.clone();
        Callback::from(move |_: MouseEvent| editor.dispatch(EditorAction::ResetCanvasDefaults))
    };

    let on_close = props.on_close.clone();

    // The scrim and the popover are siblings, not parent/child: a click
    // inside the popover bubbles to its own ancestors, never to the scrim,
    // so closing-on-outside-click falls out for free without needing to
    // stop propagation on every inner control.
    //
    // `prevent_default` on the scrim's mousedown matters beyond just
    // dismissing the popover cleanly: the scrim itself isn't focusable, so
    // without this a click on it blurs whatever *was* focused (e.g. the
    // background hex field) straight to `<body>` — which sits outside
    // `.app-shell`'s subtree, so `app.rs`'s keydown handler (Ctrl+Z/Y,
    // Delete, arrow-nudge, ...) stops receiving events entirely until the
    // user clicks back into the canvas. Blocking the browser's default
    // focus-stealing behavior here keeps focus wherever it already was,
    // so Undo/Redo keep working right after closing the panel.
    let on_scrim_mousedown = Callback::from(move |event: MouseEvent| {
        event.prevent_default();
        on_close.emit(());
    });

    html! {
        <>
            <div class="canvas-settings-scrim" onmousedown={on_scrim_mousedown}></div>
            <div class="canvas-settings">
                <div class="canvas-settings__heading-row">
                    <h3 class="right-panel__subheading">{"Canvas Size"}</h3>
                    <button type="button" class="canvas-settings__reset" title="Restore default size and background" onclick={on_reset_defaults}>
                        {"Reset to Default"}
                    </button>
                </div>

                <div class="canvas-settings__dimensions">
                    <label class="right-panel__field">
                        <span class="right-panel__field-label">{"Width"}</span>
                        <span class="canvas-settings__unit-wrap">
                            <input class="right-panel__field-input canvas-settings__unit-input" type="text" value={format!("{:.0}", canvas_width)} onchange={on_width_change} />
                            <span class="canvas-settings__unit">{"px"}</span>
                        </span>
                    </label>
                    <button
                        type="button"
                        class="canvas-settings__lock"
                        title={if *aspect_locked { "Unlock aspect ratio" } else { "Lock aspect ratio" }}
                        onclick={on_lock_toggle}
                    >
                        { if *aspect_locked { "🔒" } else { "🔓" } }
                    </button>
                    <label class="right-panel__field">
                        <span class="right-panel__field-label">{"Height"}</span>
                        <span class="canvas-settings__unit-wrap">
                            <input class="right-panel__field-input canvas-settings__unit-input" type="text" value={format!("{:.0}", canvas_height)} onchange={on_height_change} />
                            <span class="canvas-settings__unit">{"px"}</span>
                        </span>
                    </label>
                </div>

                <div class="right-panel__field right-panel__field--wide">
                    <span class="right-panel__field-label">{"Aspect Ratio"}</span>
                    <input class="right-panel__field-input" type="text" value={format_aspect_ratio(canvas_width, canvas_height)} disabled={true} />
                </div>

                <div class="right-panel__field right-panel__field--wide">
                    <span class="right-panel__field-label">{"Preset"}</span>
                    <input class="right-panel__field-input" type="text" value={matched_preset_label(canvas_width, canvas_height)} disabled={true} />
                </div>

                <label class="canvas-settings__checkbox-row">
                    <input type="checkbox" class="right-panel__checkbox" checked={*scale_content} onchange={on_scale_content_toggle} />
                    <span>{"Scale content with canvas"}</span>
                </label>

                <div class="right-panel__swatch-row">
                    <span class="right-panel__field-label">{"Background"}</span>
                    <ColorField label="" color={Some(background_color)} on_live={on_background_live} on_commit={on_background_commit} />
                </div>

                <h3 class="right-panel__subheading right-panel__subheading--spaced">{"Social"}</h3>
                <div class="canvas-settings__presets">
                    { for SOCIAL_PRESETS.iter().map(|item| render_preset_button(&editor, *scale_content, item)) }
                </div>

                <h3 class="right-panel__subheading right-panel__subheading--spaced">{"Print"}</h3>
                <div class="canvas-settings__presets">
                    { for PRINT_PRESETS.iter().map(|item| render_preset_button(&editor, *scale_content, item)) }
                </div>
            </div>
        </>
    }
}
