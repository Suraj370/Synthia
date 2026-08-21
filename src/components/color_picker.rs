//! A compact, dependency-free color picker: a swatch + hex input that
//! expands into a popover with a saturation/value area, a hue slider, an
//! alpha slider, hex/RGB/alpha fields, and (where the browser supports it)
//! an eyedropper. Used identically by the shape Fill/Stroke controls and
//! the text Fill control in `right_panel.rs` — the one color-editing
//! surface in Synthia.
//!
//! Dragging in the SV area or a slider mirrors how canvas object drags
//! work (`canvas_area.rs`): every pointer move calls `on_live` (applied to
//! the document immediately, not recorded), and releasing the mouse calls
//! `on_commit` once with the color from *before* the gesture started and
//! the final color, collapsing the whole drag into a single undo step.
//! Hex/RGB/alpha field edits and the eyedropper aren't gestures — they
//! call `on_commit` directly.

use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Element, HtmlInputElement, MouseEvent};
use yew::prelude::*;

use crate::color::{hsv_to_rgb, parse_hex, Hsv};
use crate::model::Color;

const SV_WIDTH: f64 = 168.0;
const SV_HEIGHT: f64 = 110.0;
const SLIDER_WIDTH: f64 = 168.0;

#[derive(Clone, Copy, PartialEq)]
enum DragKind {
    SatVal,
    Hue,
    Alpha,
}

/// `(kind, color as of gesture start, hsv as of gesture start)` — the hsv
/// snapshot is what keeps e.g. a saturation/value drag from drifting the
/// hue on every recompute (RGB<->HSV isn't perfectly stable for grays).
type DragState = (DragKind, Color, Hsv);

#[derive(Properties, PartialEq)]
pub struct ColorFieldProps {
    pub label: AttrValue,
    pub color: Option<Color>,
    pub on_live: Callback<Color>,
    pub on_commit: Callback<(Color, Color)>,
    #[prop_or(false)]
    pub disabled: bool,
}

fn eyedropper_available() -> bool {
    web_sys::window().is_some_and(|window| js_sys::Reflect::has(&window, &JsValue::from_str("EyeDropper")).unwrap_or(false))
}

/// Opens the browser's native `EyeDropper` (Chromium/WebView2 only — the
/// same engine Tauri uses on Windows) via raw JS interop rather than typed
/// `web_sys` bindings, so this doesn't need a new `web-sys` feature flag.
/// Samples anywhere on screen, not just the Synthia canvas — the API has no
/// narrower mode — which in practice covers "sample a color from the
/// canvas" and then some.
async fn sample_eyedropper() -> Option<(u8, u8, u8)> {
    let window = web_sys::window()?;
    let ctor = js_sys::Reflect::get(&window, &JsValue::from_str("EyeDropper")).ok()?;
    let ctor: js_sys::Function = ctor.dyn_into().ok()?;
    let instance = js_sys::Reflect::construct(&ctor, &js_sys::Array::new()).ok()?;
    let open_fn = js_sys::Reflect::get(&instance, &JsValue::from_str("open")).ok()?;
    let open_fn: js_sys::Function = open_fn.dyn_into().ok()?;
    let promise: js_sys::Promise = open_fn.call0(&instance).ok()?.dyn_into().ok()?;
    let result = wasm_bindgen_futures::JsFuture::from(promise).await.ok()?;
    let hex_value = js_sys::Reflect::get(&result, &JsValue::from_str("sRGBHex")).ok()?;
    parse_hex(&hex_value.as_string()?)
}

#[function_component(ColorField)]
pub fn color_field(props: &ColorFieldProps) -> Html {
    let open = use_state(|| false);
    let drag = use_state(|| None::<DragState>);
    let popover_pos = use_state(|| (0.0_f64, 0.0_f64));
    let has_eyedropper = use_state(eyedropper_available);
    let swatch_ref = use_node_ref();
    let sv_ref = use_node_ref();
    let hue_ref = use_node_ref();
    let alpha_ref = use_node_ref();

    let disabled = props.disabled || props.color.is_none();
    let current = props.color.unwrap_or(Color::rgb(0, 0, 0));
    let is_open = *open && !disabled;

    // While a gesture is in progress the drag's own hsv snapshot is the
    // source of truth (see `DragState` docs); otherwise derive fresh from
    // the live color every render, so an external change (undo, another
    // field) is always reflected.
    let display_hsv = drag.map(|(_, _, hsv)| hsv).unwrap_or_else(|| current.to_hsv());

    let toggle_open = {
        let open = open.clone();
        let swatch_ref = swatch_ref.clone();
        let popover_pos = popover_pos.clone();
        Callback::from(move |_: MouseEvent| {
            if !*open {
                if let Some(el) = swatch_ref.cast::<Element>() {
                    let rect = el.get_bounding_client_rect();
                    let window_width = web_sys::window().and_then(|w| w.inner_width().ok()).and_then(|v| v.as_f64()).unwrap_or(1280.0);
                    popover_pos.set((window_width - rect.right(), rect.bottom() + 6.0));
                }
            }
            open.set(!*open);
        })
    };

    let close = {
        let open = open.clone();
        Callback::from(move |_: MouseEvent| open.set(false))
    };

    let start_drag = |kind: DragKind| {
        let drag = drag.clone();
        Callback::from(move |event: MouseEvent| {
            event.stop_propagation();
            drag.set(Some((kind, current, current.to_hsv())));
        })
    };

    let on_scrim_move = {
        let drag = drag.clone();
        let on_live = props.on_live.clone();
        let sv_ref = sv_ref.clone();
        let hue_ref = hue_ref.clone();
        let alpha_ref = alpha_ref.clone();
        Callback::from(move |event: MouseEvent| {
            let Some((kind, before, start_hsv)) = *drag else { return };
            let new_color = match kind {
                DragKind::SatVal => {
                    let Some(el) = sv_ref.cast::<Element>() else { return };
                    let rect = el.get_bounding_client_rect();
                    if rect.width() == 0.0 || rect.height() == 0.0 {
                        return;
                    }
                    let s = ((event.client_x() as f64 - rect.left()) / rect.width()).clamp(0.0, 1.0);
                    let v = 1.0 - ((event.client_y() as f64 - rect.top()) / rect.height()).clamp(0.0, 1.0);
                    let (r, g, b) = hsv_to_rgb(Hsv { h: start_hsv.h, s, v });
                    Color { r, g, b, a: before.a }
                }
                DragKind::Hue => {
                    let Some(el) = hue_ref.cast::<Element>() else { return };
                    let rect = el.get_bounding_client_rect();
                    if rect.width() == 0.0 {
                        return;
                    }
                    let h = ((event.client_x() as f64 - rect.left()) / rect.width()).clamp(0.0, 1.0) * 360.0;
                    let (r, g, b) = hsv_to_rgb(Hsv { h, s: start_hsv.s, v: start_hsv.v });
                    Color { r, g, b, a: before.a }
                }
                DragKind::Alpha => {
                    let Some(el) = alpha_ref.cast::<Element>() else { return };
                    let rect = el.get_bounding_client_rect();
                    if rect.width() == 0.0 {
                        return;
                    }
                    let a = ((event.client_x() as f64 - rect.left()) / rect.width()).clamp(0.0, 1.0);
                    Color { a, ..before }
                }
            };
            on_live.emit(new_color);
        })
    };

    let on_scrim_up = {
        let drag = drag.clone();
        let on_commit = props.on_commit.clone();
        Callback::from(move |_: MouseEvent| {
            if let Some((_, before, _)) = *drag {
                if before != current {
                    on_commit.emit((before, current));
                }
                drag.set(None);
            }
        })
    };

    let on_hex_change = {
        let on_commit = props.on_commit.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            if let Some((r, g, b)) = parse_hex(&input.value()) {
                let after = Color { r, g, b, a: current.a };
                if after != current {
                    on_commit.emit((current, after));
                }
            }
        })
    };

    let channel_change = |set: fn(&mut Color, u8)| {
        let on_commit = props.on_commit.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            if let Ok(parsed) = input.value().trim().parse::<i64>() {
                let mut after = current;
                set(&mut after, parsed.clamp(0, 255) as u8);
                if after != current {
                    on_commit.emit((current, after));
                }
            }
        })
    };

    let on_alpha_field_change = {
        let on_commit = props.on_commit.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            if let Ok(parsed) = input.value().trim().parse::<f64>() {
                let after = current.with_alpha(parsed / 100.0);
                if after != current {
                    on_commit.emit((current, after));
                }
            }
        })
    };

    let on_eyedropper = {
        let on_commit = props.on_commit.clone();
        Callback::from(move |_: MouseEvent| {
            let on_commit = on_commit.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Some((r, g, b)) = sample_eyedropper().await {
                    let after = Color { r, g, b, a: current.a };
                    if after != current {
                        on_commit.emit((current, after));
                    }
                }
            });
        })
    };

    let swatch_style = format!(
        "background-image: linear-gradient({0}, {0}), var(--color-field-checker); background-size: 100% 100%, 8px 8px;",
        current.to_css()
    );
    let (hue_r, hue_g, hue_b) = hsv_to_rgb(Hsv { h: display_hsv.h, s: 1.0, v: 1.0 });
    let sv_hue_color = Color::rgb(hue_r, hue_g, hue_b);
    let sv_area_style = format!(
        "width:{SV_WIDTH}px; height:{SV_HEIGHT}px; background-image: linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, {});",
        sv_hue_color.to_css()
    );
    let sv_thumb_style = format!("left:{}px; top:{}px;", display_hsv.s * SV_WIDTH, (1.0 - display_hsv.v) * SV_HEIGHT);
    let hue_area_style = format!("width:{SLIDER_WIDTH}px;");
    let hue_thumb_style = format!("left:{}px;", (display_hsv.h / 360.0) * SLIDER_WIDTH);
    let alpha_gradient_color = format!("rgba({}, {}, {}, 1)", current.r, current.g, current.b);
    let alpha_area_style = format!(
        "width:{SLIDER_WIDTH}px; background-image: linear-gradient(to right, transparent, {}), var(--color-field-checker); background-size: 100% 100%, 8px 8px;",
        alpha_gradient_color
    );
    let alpha_thumb_style = format!("left:{}px;", current.a * SLIDER_WIDTH);
    let popover_style = format!("right:{}px; top:{}px;", popover_pos.0, popover_pos.1);

    html! {
        <div class="color-field">
            { if !props.label.is_empty() {
                html! { <span class="right-panel__field-label">{ props.label.clone() }</span> }
            } else {
                html! {}
            } }
            <button
                ref={swatch_ref}
                type="button"
                class="color-field__swatch"
                style={swatch_style}
                {disabled}
                onclick={toggle_open}
            ></button>
            <input
                class="right-panel__field-input color-field__hex-input"
                type="text"
                value={current.to_hex()}
                placeholder="—"
                disabled={disabled}
                onchange={on_hex_change.clone()}
            />
            { if is_open {
                html! {
                    <div class="color-picker__scrim" onmousedown={close} onmousemove={on_scrim_move} onmouseup={on_scrim_up}>
                        <div class="color-picker__popover" style={popover_style} onmousedown={Callback::from(|e: MouseEvent| e.stop_propagation())}>
                            <div ref={sv_ref} class="color-picker__sv-area" style={sv_area_style} onmousedown={start_drag(DragKind::SatVal)}>
                                <div class="color-picker__sv-thumb" style={sv_thumb_style}></div>
                            </div>
                            <div ref={hue_ref} class="color-picker__hue-slider" style={hue_area_style} onmousedown={start_drag(DragKind::Hue)}>
                                <div class="color-picker__slider-thumb" style={hue_thumb_style}></div>
                            </div>
                            <div ref={alpha_ref} class="color-picker__alpha-slider" style={alpha_area_style} onmousedown={start_drag(DragKind::Alpha)}>
                                <div class="color-picker__slider-thumb" style={alpha_thumb_style}></div>
                            </div>
                            <div class="color-picker__fields">
                                <input
                                    class="color-picker__hex-field"
                                    type="text"
                                    value={current.to_hex()}
                                    onchange={on_hex_change}
                                />
                                <input class="color-picker__channel-field" type="text" value={current.r.to_string()} onchange={channel_change(|c, v| c.r = v)} />
                                <input class="color-picker__channel-field" type="text" value={current.g.to_string()} onchange={channel_change(|c, v| c.g = v)} />
                                <input class="color-picker__channel-field" type="text" value={current.b.to_string()} onchange={channel_change(|c, v| c.b = v)} />
                                <input
                                    class="color-picker__channel-field"
                                    type="text"
                                    value={format!("{:.0}", current.a * 100.0)}
                                    onchange={on_alpha_field_change}
                                />
                            </div>
                            { if *has_eyedropper {
                                html! {
                                    <button type="button" class="color-picker__eyedropper" title="Pick a color from the screen" onclick={on_eyedropper}>
                                        {"◎ Eyedropper"}
                                    </button>
                                }
                            } else {
                                html! {}
                            } }
                        </div>
                    </div>
                }
            } else {
                html! {}
            } }
        </div>
    }
}
