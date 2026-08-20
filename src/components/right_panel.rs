use std::collections::BTreeSet;

use web_sys::{DragEvent, HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;

use crate::editor_state::{use_editor, AlignMode, DistributeAxis, EditorAction, EditorContext};
use crate::image_import::{files_from_input, import_image_file, ImportTarget};
use crate::model::{
    DesignDocument, FontFamily, FontStyle, FontWeight, Geometry, ImageFit, ImageProperties, ObjectId, ObjectKind, TextAlign,
    TextDecoration, TextProperties, MIN_OBJECT_SIZE,
};

fn format_number(value: f64) -> String {
    format!("{value:.0}")
}

/// Like `format_number` but keeps up to `precision` decimals (trimming
/// trailing zeros) instead of always rounding to a whole number — needed
/// for typography values like line height (1.2) or letter spacing (0.5)
/// where `format_number`'s `.0` would silently discard the fraction.
fn format_number_precise(value: f64, precision: usize) -> String {
    let formatted = format!("{value:.precision$}");
    if formatted.contains('.') {
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        if trimmed.is_empty() { "0".to_string() } else { trimmed.to_string() }
    } else {
        formatted
    }
}

fn kind_icon(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Rectangle => "▭",
        ObjectKind::Ellipse => "○",
        ObjectKind::Text(_) => "T",
        ObjectKind::Image(_) => "▧",
        ObjectKind::Group => "▤",
    }
}

fn render_field(label: &'static str, value: Option<f64>, disabled: bool, wide: bool, on_commit: Callback<f64>) -> Html {
    let onchange = Callback::from(move |event: Event| {
        let input: HtmlInputElement = event.target_unchecked_into();
        if let Ok(parsed) = input.value().trim().parse::<f64>() {
            on_commit.emit(parsed);
        }
    });
    let class = if wide { "right-panel__field right-panel__field--wide" } else { "right-panel__field" };

    html! {
        <label key={label} {class}>
            <span class="right-panel__field-label">{label}</span>
            <input
                class="right-panel__field-input"
                type="text"
                value={value.map(format_number).unwrap_or_default()}
                placeholder="—"
                {disabled}
                {onchange}
            />
        </label>
    }
}

/// Like `render_field`, but formats with up to `precision` decimal places
/// instead of always rounding to a whole number.
fn render_field_precise(label: &'static str, value: Option<f64>, disabled: bool, wide: bool, precision: usize, on_commit: Callback<f64>) -> Html {
    let onchange = Callback::from(move |event: Event| {
        let input: HtmlInputElement = event.target_unchecked_into();
        if let Ok(parsed) = input.value().trim().parse::<f64>() {
            on_commit.emit(parsed);
        }
    });
    let class = if wide { "right-panel__field right-panel__field--wide" } else { "right-panel__field" };

    html! {
        <label key={label} {class}>
            <span class="right-panel__field-label">{label}</span>
            <input
                class="right-panel__field-input"
                type="text"
                value={value.map(|v| format_number_precise(v, precision)).unwrap_or_default()}
                placeholder="—"
                {disabled}
                {onchange}
            />
        </label>
    }
}

/// Builds a Properties field bound to one `Geometry` component: `get` reads
/// the display value, `set` writes a committed edit back onto a copy of the
/// selected object's geometry before it's dispatched as one undoable step.
fn geometry_field(
    editor: &EditorContext,
    selected: Option<(ObjectId, Geometry)>,
    label: &'static str,
    wide: bool,
    get: fn(&Geometry) -> f64,
    set: fn(&mut Geometry, f64),
) -> Html {
    let value = selected.map(|(_, g)| get(&g));
    let editor = editor.clone();
    let on_commit = Callback::from(move |input: f64| {
        if let Some((id, before)) = selected {
            let mut after = before;
            set(&mut after, input);
            editor.dispatch(EditorAction::CommitGeometries(vec![(id, before, after)]));
        }
    });
    render_field(label, value, selected.is_none(), wide, on_commit)
}

/// A non-interactive appearance swatch. There's no per-object fill/stroke
/// data in the model yet, so this reflects the one fixed color every shape
/// actually renders with — honest about not being editable rather than
/// faking a control with nothing behind it.
fn render_swatch_row(label: &'static str, color: &str) -> Html {
    html! {
        <div class="right-panel__swatch-row">
            <span class="right-panel__field-label">{label}</span>
            <span class="right-panel__swatch" style={format!("background-color: {color}")}></span>
        </div>
    }
}

/// A `<select>`-backed Properties field bound to one text property: `get`
/// reads the current option value, `set` applies the chosen value back
/// onto a copy of the selected text's properties before it's dispatched as
/// one undoable step (mirrors `geometry_field`'s before/after pattern).
fn text_select_field(
    editor: &EditorContext,
    selected: &Option<(ObjectId, TextProperties)>,
    label: &'static str,
    wide: bool,
    options: &[&'static str],
    current: &str,
    set: fn(&mut TextProperties, &str),
) -> Html {
    let class = if wide { "right-panel__field right-panel__field--wide" } else { "right-panel__field" };
    let disabled = selected.is_none();
    let editor = editor.clone();
    let selected = selected.clone();
    let onchange = Callback::from(move |event: Event| {
        let select: HtmlSelectElement = event.target_unchecked_into();
        let value = select.value();
        if let Some((id, before)) = &selected {
            let mut after = before.clone();
            set(&mut after, &value);
            editor.dispatch(EditorAction::CommitTextProperties { id: *id, before: before.clone(), after });
        }
    });

    html! {
        <label key={label} {class}>
            <span class="right-panel__field-label">{label}</span>
            <select class="right-panel__select" {disabled} {onchange}>
                { for options.iter().map(|option| html! {
                    <option key={*option} value={*option} selected={*option == current}>{ *option }</option>
                }) }
            </select>
        </label>
    }
}

/// A row of toggle buttons bound to one text property — used for the
/// short, mutually-exclusive choices (Style, Alignment, Decoration) that
/// read more clearly as buttons than as a dropdown.
fn text_segmented_field(
    editor: &EditorContext,
    selected: &Option<(ObjectId, TextProperties)>,
    label: &'static str,
    options: &[(&'static str, &'static str)],
    current: &str,
    set: fn(&mut TextProperties, &str),
) -> Html {
    let disabled = selected.is_none();
    html! {
        <div class="right-panel__field right-panel__field--wide">
            <span class="right-panel__field-label">{label}</span>
            <div class="right-panel__segmented">
                { for options.iter().map(|(value, display)| {
                    let editor = editor.clone();
                    let selected = selected.clone();
                    let value = *value;
                    let active = value == current;
                    let onclick = Callback::from(move |_| {
                        if let Some((id, before)) = &selected {
                            let mut after = before.clone();
                            set(&mut after, value);
                            editor.dispatch(EditorAction::CommitTextProperties { id: *id, before: before.clone(), after });
                        }
                    });
                    let class = if active {
                        "right-panel__segmented-button right-panel__segmented-button--active"
                    } else {
                        "right-panel__segmented-button"
                    };
                    html! { <button key={value} type="button" {class} {disabled} {onclick}>{ *display }</button> }
                }) }
            </div>
        </div>
    }
}

/// A numeric Properties field bound to one text property, same pattern as
/// `geometry_field` but reading/writing `TextProperties` instead.
fn text_number_field(
    editor: &EditorContext,
    selected: &Option<(ObjectId, TextProperties)>,
    label: &'static str,
    wide: bool,
    precision: usize,
    get: fn(&TextProperties) -> f64,
    set: fn(&mut TextProperties, f64),
) -> Html {
    let value = selected.as_ref().map(|(_, props)| get(props));
    let disabled = selected.is_none();
    let editor = editor.clone();
    let selected = selected.clone();
    let on_commit = Callback::from(move |input: f64| {
        if let Some((id, before)) = &selected {
            let mut after = before.clone();
            set(&mut after, input);
            editor.dispatch(EditorAction::CommitTextProperties { id: *id, before: before.clone(), after });
        }
    });
    render_field_precise(label, value, disabled, wide, precision, on_commit)
}

/// The Fill color control for text — unlike `render_swatch_row` (which is
/// just an honest placeholder for kinds with no real per-object color
/// data yet), text objects do have a real `fill`, so this is a working
/// `<input type="color">` bound to it.
fn text_fill_field(editor: &EditorContext, selected: &Option<(ObjectId, TextProperties)>) -> Html {
    let value = selected.as_ref().map(|(_, props)| props.fill.clone()).unwrap_or_else(|| "#000000".to_string());
    let disabled = selected.is_none();
    let editor = editor.clone();
    let selected = selected.clone();
    let onchange = Callback::from(move |event: Event| {
        let input: HtmlInputElement = event.target_unchecked_into();
        if let Some((id, before)) = &selected {
            let mut after = before.clone();
            after.fill = input.value();
            editor.dispatch(EditorAction::CommitTextProperties { id: *id, before: before.clone(), after });
        }
    });

    html! {
        <div class="right-panel__swatch-row">
            <span class="right-panel__field-label">{"Fill"}</span>
            <input type="color" class="right-panel__color-input" {value} {disabled} {onchange} />
        </div>
    }
}

/// Typography + Appearance sections shown in place of the generic
/// swatch-based Appearance block whenever the single selected object is
/// text. Every control commits immediately as its own undo step, the same
/// as every other Properties panel field.
fn render_typography_section(editor: &EditorContext, selected: &Option<(ObjectId, TextProperties)>) -> Html {
    let font_family_label = selected.as_ref().map(|(_, p)| p.font_family.label()).unwrap_or_else(|| FontFamily::default().label());
    let font_weight_label = selected.as_ref().map(|(_, p)| p.font_weight.label()).unwrap_or_else(|| FontWeight::default().label());
    let style_value = selected.as_ref().map(|(_, p)| if p.font_style == FontStyle::Italic { "Italic" } else { "Regular" }).unwrap_or("Regular");
    let align_value = selected
        .as_ref()
        .map(|(_, p)| match p.text_align {
            TextAlign::Left => "Left",
            TextAlign::Center => "Center",
            TextAlign::Right => "Right",
        })
        .unwrap_or("Left");
    let decoration_value = selected
        .as_ref()
        .map(|(_, p)| match p.text_decoration {
            TextDecoration::None => "None",
            TextDecoration::Underline => "Underline",
            TextDecoration::Strikethrough => "Strikethrough",
        })
        .unwrap_or("None");

    let font_options: Vec<&'static str> = FontFamily::ALL.iter().map(|f| f.label()).collect();
    let weight_options: Vec<&'static str> = FontWeight::ALL.iter().map(|w| w.label()).collect();

    html! {
        <>
            <div class="right-panel__subsection">
                <h3 class="right-panel__subheading">{"Text"}</h3>
                <div class="right-panel__fields">
                    { text_select_field(editor, selected, "Font", true, &font_options, font_family_label, |props, value| {
                        if let Some(family) = FontFamily::from_label(value) {
                            props.font_family = family;
                        }
                    }) }
                    { text_select_field(editor, selected, "Weight", false, &weight_options, font_weight_label, |props, value| {
                        if let Some(weight) = FontWeight::from_label(value) {
                            props.font_weight = weight;
                        }
                    }) }
                    { text_number_field(editor, selected, "Size", false, 0, |p| p.font_size, |p, v| p.font_size = v.max(1.0)) }
                    { text_segmented_field(editor, selected, "Style", &[("Regular", "Regular"), ("Italic", "Italic")], style_value, |props, value| {
                        props.font_style = if value == "Italic" { FontStyle::Italic } else { FontStyle::Regular };
                    }) }
                    { text_segmented_field(editor, selected, "Alignment", &[("Left", "Left"), ("Center", "Center"), ("Right", "Right")], align_value, |props, value| {
                        props.text_align = match value {
                            "Center" => TextAlign::Center,
                            "Right" => TextAlign::Right,
                            _ => TextAlign::Left,
                        };
                    }) }
                    { text_number_field(editor, selected, "Line Height", false, 2, |p| p.line_height, |p, v| p.line_height = v.max(0.5)) }
                    { text_number_field(editor, selected, "Letter Spacing", false, 2, |p| p.letter_spacing, |p, v| p.letter_spacing = v) }
                </div>
            </div>
            <div class="right-panel__subsection">
                <h3 class="right-panel__subheading">{"Appearance"}</h3>
                { text_fill_field(editor, selected) }
                { text_segmented_field(editor, selected, "Decoration", &[("None", "None"), ("Underline", "Underline"), ("Strikethrough", "Strike")], decoration_value, |props, value| {
                    props.text_decoration = match value {
                        "Underline" => TextDecoration::Underline,
                        "Strikethrough" => TextDecoration::Strikethrough,
                        _ => TextDecoration::None,
                    };
                }) }
            </div>
        </>
    }
}

/// The X/Y/W/H/Rotation block shared by every single-object inspector
/// (text and shape alike) — the one part of the Properties panel that
/// never depended on object kind in the first place.
fn render_transform_section(editor: &EditorContext, selected: Option<(ObjectId, Geometry)>) -> Html {
    html! {
        <div class="right-panel__subsection">
            <h3 class="right-panel__subheading">{"Transform"}</h3>
            <div class="right-panel__fields">
                { geometry_field(editor, selected, "X", false, |g| g.x, |g, v| g.x = v) }
                { geometry_field(editor, selected, "Y", false, |g| g.y, |g, v| g.y = v) }
                { geometry_field(editor, selected, "W", false, |g| g.width, |g, v| g.width = v.max(MIN_OBJECT_SIZE)) }
                { geometry_field(editor, selected, "H", false, |g| g.height, |g, v| g.height = v.max(MIN_OBJECT_SIZE)) }
                { geometry_field(editor, selected, "Rotation", true, |g| g.rotation, |g, v| g.rotation = v) }
            </div>
        </div>
    }
}

/// Properties content for when nothing is selected.
fn render_empty_inspector() -> Html {
    render_empty_state("No selection", "Select an object to edit its properties.")
}

/// Properties content for a single selected text object: Text (typography)
/// + Appearance (fill/decoration) + Opacity + Transform. Typography only
/// ever appears here — every other inspector below is typography-free.
fn render_text_inspector(editor: &EditorContext, selected_text: &Option<(ObjectId, TextProperties)>, selected: Option<(ObjectId, Geometry)>) -> Html {
    html! {
        <>
            { render_typography_section(editor, selected_text) }
            <div class="right-panel__subsection">
                <h3 class="right-panel__subheading">{"Opacity"}</h3>
                <div class="right-panel__fields right-panel__fields--single">
                    { geometry_field(editor, selected, "Opacity", true, |g| g.opacity * 100.0, |g, v| g.opacity = (v / 100.0).clamp(0.0, 1.0)) }
                </div>
            </div>
            { render_transform_section(editor, selected) }
        </>
    }
}

/// A row of toggle buttons bound to one image property — same shape as
/// `text_segmented_field`, kept as its own small copy rather than a shared
/// generic since the two properties structs have nothing else in common.
fn image_segmented_field(
    editor: &EditorContext,
    selected: &Option<(ObjectId, ImageProperties)>,
    label: &'static str,
    options: &[(&'static str, &'static str)],
    current: &str,
    set: fn(&mut ImageProperties, &str),
) -> Html {
    let disabled = selected.is_none();
    html! {
        <div class="right-panel__field right-panel__field--wide">
            <span class="right-panel__field-label">{label}</span>
            <div class="right-panel__segmented">
                { for options.iter().map(|(value, display)| {
                    let editor = editor.clone();
                    let selected = *selected;
                    let value = *value;
                    let active = value == current;
                    let onclick = Callback::from(move |_| {
                        if let Some((id, before)) = selected {
                            let mut after = before;
                            set(&mut after, value);
                            editor.dispatch(EditorAction::CommitImageProperties { id, before, after });
                        }
                    });
                    let class = if active {
                        "right-panel__segmented-button right-panel__segmented-button--active"
                    } else {
                        "right-panel__segmented-button"
                    };
                    html! { <button key={value} type="button" {class} {disabled} {onclick}>{ *display }</button> }
                }) }
            </div>
        </div>
    }
}

/// Properties content for a single selected image object: Image
/// (filename, original dimensions, Replace) + Fit + Opacity + Transform.
fn render_image_inspector(
    editor: &EditorContext,
    document: &DesignDocument,
    selected_image: &Option<(ObjectId, ImageProperties)>,
    selected: Option<(ObjectId, Geometry)>,
    replace_input_ref: NodeRef,
) -> Html {
    let asset = selected_image.and_then(|(_, props)| document.assets.get(props.asset_id));
    let filename = asset.map(|a| a.filename.clone()).unwrap_or_else(|| "—".to_string());
    let original_dimensions = asset.map(|a| format!("{:.0} × {:.0}", a.width, a.height));
    let fit_value = selected_image
        .map(|(_, props)| match props.fit {
            ImageFit::Fill => "Fill",
            ImageFit::Fit => "Fit",
            ImageFit::Crop => "Crop",
        })
        .unwrap_or("Fill");

    let on_replace_click = {
        let replace_input_ref = replace_input_ref.clone();
        Callback::from(move |_| {
            if let Some(input) = replace_input_ref.cast::<HtmlInputElement>() {
                input.click();
            }
        })
    };

    let on_replace_change = {
        let editor = editor.clone();
        let selected_image = *selected_image;
        Callback::from(move |event: Event| {
            let Some((id, _)) = selected_image else { return };
            let input: HtmlInputElement = event.target_unchecked_into();
            for file in files_from_input(&input) {
                import_image_file(editor.clone(), file, ImportTarget::Replace { object_id: id });
            }
            // Clearing the input lets picking the *same* file again still
            // fire a change event next time.
            input.set_value("");
        })
    };

    html! {
        <>
            <div class="right-panel__subsection">
                <h3 class="right-panel__subheading">{"Image"}</h3>
                <div class="right-panel__image-filename">{ filename }</div>
                { if let Some(dims) = original_dimensions {
                    html! { <div class="right-panel__image-dimensions">{ format!("Original: {dims}") }</div> }
                } else {
                    html! {}
                } }
                <button type="button" class="right-panel__replace-button" onclick={on_replace_click}>{"Replace Image"}</button>
                <input
                    ref={replace_input_ref}
                    type="file"
                    accept="image/png,image/jpeg,image/webp"
                    class="hidden-file-input"
                    onchange={on_replace_change}
                />
            </div>
            <div class="right-panel__subsection">
                <h3 class="right-panel__subheading">{"Fit"}</h3>
                { image_segmented_field(editor, selected_image, "Fit", &[("Fill", "Fill"), ("Fit", "Fit"), ("Crop", "Crop")], fit_value, |props, value| {
                    props.fit = match value {
                        "Fit" => ImageFit::Fit,
                        "Crop" => ImageFit::Crop,
                        _ => ImageFit::Fill,
                    };
                }) }
            </div>
            <div class="right-panel__subsection">
                <h3 class="right-panel__subheading">{"Opacity"}</h3>
                <div class="right-panel__fields right-panel__fields--single">
                    { geometry_field(editor, selected, "Opacity", true, |g| g.opacity * 100.0, |g, v| g.opacity = (v / 100.0).clamp(0.0, 1.0)) }
                </div>
            </div>
            { render_transform_section(editor, selected) }
        </>
    }
}

/// Properties content for a single selected non-text, non-group object
/// (rectangle, ellipse): Transform + Appearance
/// (fill/stroke/opacity). No typography controls.
fn render_shape_inspector(editor: &EditorContext, selected: Option<(ObjectId, Geometry)>) -> Html {
    html! {
        <>
            { render_transform_section(editor, selected) }
            <div class="right-panel__subsection">
                <h3 class="right-panel__subheading">{"Appearance"}</h3>
                { render_swatch_row("Fill", "var(--bg-raised)") }
                { render_swatch_row("Stroke", "var(--border-strong)") }
                <div class="right-panel__fields right-panel__fields--single">
                    { geometry_field(editor, selected, "Opacity", true, |g| g.opacity * 100.0, |g, v| g.opacity = (v / 100.0).clamp(0.0, 1.0)) }
                </div>
            </div>
        </>
    }
}

/// Properties content for 2+ selected objects: a count, a read-only
/// bounding-box Transform summary (there's no single object's geometry to
/// bind editable fields to), and the existing align/distribute controls.
/// Never shows typography, even if every selected object happens to be text.
fn render_multi_selection_inspector(editor: &EditorContext) -> Html {
    let count = editor.selected_ids.len();
    let ids: Vec<ObjectId> = editor.selected_ids.iter().copied().collect();
    let bbox = editor.document.bounding_box(&ids);
    let readonly_field = |label: &'static str, value: Option<f64>| render_field(label, value, true, false, Callback::from(|_: f64| {}));

    html! {
        <>
            <div class="right-panel__subsection">
                <h3 class="right-panel__subheading">{"Multiple Selection"}</h3>
                <span class="right-panel__multi-count">{ format!("{count} objects selected") }</span>
            </div>
            <div class="right-panel__subsection">
                <h3 class="right-panel__subheading">{"Transform"}</h3>
                <div class="right-panel__fields">
                    { readonly_field("X", bbox.map(|b| b.x)) }
                    { readonly_field("Y", bbox.map(|b| b.y)) }
                    { readonly_field("W", bbox.map(|b| b.width)) }
                    { readonly_field("H", bbox.map(|b| b.height)) }
                </div>
            </div>
            { render_align_section(editor) }
        </>
    }
}

fn render_empty_state(title: &str, description: &str) -> Html {
    html! {
        <div class="right-panel__empty-state">
            <span class="right-panel__empty-state-title">{title}</span>
            <span class="right-panel__empty-state-description">{description}</span>
        </div>
    }
}

/// Renders one nesting level of the Layers tree (newest-drawn-first, same
/// convention as the flat list before grouping existed) and recurses into
/// any expanded group's children, indenting them underneath it.
fn render_layer_rows(
    document: &DesignDocument,
    editor: &EditorContext,
    dragged: &UseStateHandle<Option<ObjectId>>,
    parent: Option<ObjectId>,
    depth: usize,
) -> Vec<Html> {
    document
        .children_of(parent)
        .rev()
        .flat_map(|object| {
            let id = object.id;
            let own_parent = object.parent_id;
            let is_group = object.kind.is_group();
            let is_expanded = editor.expanded_groups.contains(&id);
            let class = if editor.is_selected(id) {
                "right-panel__layer right-panel__layer--active"
            } else {
                "right-panel__layer"
            };

            let onclick = {
                let editor = editor.clone();
                Callback::from(move |event: MouseEvent| {
                    let mut selection = editor.selected_ids.clone();
                    if event.shift_key() {
                        if !selection.remove(&id) {
                            selection.insert(id);
                        }
                    } else {
                        selection = BTreeSet::from([id]);
                    }
                    editor.dispatch(EditorAction::SetSelection(selection));
                })
            };

            let toggle = if is_group {
                let editor = editor.clone();
                let onclick = Callback::from(move |event: MouseEvent| {
                    event.stop_propagation();
                    editor.dispatch(EditorAction::ToggleGroupExpanded(id));
                });
                let caret = if is_expanded { "▾" } else { "▸" };
                html! { <button type="button" class="right-panel__layer-toggle" {onclick}>{caret}</button> }
            } else {
                html! { <span class="right-panel__layer-toggle right-panel__layer-toggle--spacer"></span> }
            };

            let ondragstart = {
                let dragged = dragged.clone();
                Callback::from(move |_: DragEvent| dragged.set(Some(id)))
            };
            let ondragover = Callback::from(|event: DragEvent| event.prevent_default());
            let ondrop = {
                let dragged = dragged.clone();
                let editor = editor.clone();
                Callback::from(move |event: DragEvent| {
                    event.prevent_default();
                    if let Some(source_id) = *dragged {
                        let source_parent = editor.document.get(source_id).and_then(|o| o.parent_id);
                        if source_id != id && source_parent == own_parent {
                            if let Some(new_index) = editor.document.objects.iter().position(|o| o.id == id) {
                                editor.dispatch(EditorAction::ReorderObject { id: source_id, new_index });
                            }
                        }
                    }
                    dragged.set(None);
                })
            };

            let mut rows = vec![html! {
                <li
                    key={id}
                    {class}
                    tabindex="0"
                    style={format!("padding-left: {}px", 8 + depth * 16)}
                    draggable="true"
                    {onclick}
                    {ondragstart}
                    {ondragover}
                    {ondrop}
                >
                    { toggle }
                    <span class="right-panel__layer-icon">{kind_icon(&object.kind)}</span>
                    <span class="right-panel__layer-name">{object.name.clone()}</span>
                    <span class="right-panel__layer-visibility" title="Visible">{"●"}</span>
                </li>
            }];

            if is_group && is_expanded {
                rows.extend(render_layer_rows(document, editor, dragged, Some(id), depth + 1));
            }

            rows
        })
        .collect()
}

/// Compact alignment/distribution controls, shown in place of the usual
/// single-object Transform/Appearance fields whenever 2+ objects are
/// selected (distribute needs 3+, so those buttons stay disabled until
/// then rather than being hidden).
fn render_align_section(editor: &EditorContext) -> Html {
    let count = editor.selected_ids.len();

    let align_button = |label: &'static str, glyph: &'static str, mode: AlignMode| {
        let editor = editor.clone();
        let onclick = Callback::from(move |_| editor.dispatch(EditorAction::AlignObjects(mode)));
        html! {
            <button type="button" class="right-panel__align-button" title={label} {onclick}>{glyph}</button>
        }
    };

    let distribute_button = |label: &'static str, glyph: &'static str, axis: DistributeAxis| {
        let editor = editor.clone();
        let disabled = count < 3;
        let onclick = Callback::from(move |_| editor.dispatch(EditorAction::DistributeObjects(axis)));
        html! {
            <button type="button" class="right-panel__align-button" title={label} {disabled} {onclick}>{glyph}</button>
        }
    };

    html! {
        <div class="right-panel__subsection">
            <h3 class="right-panel__subheading">{"Align"}</h3>
            <div class="right-panel__align-row">
                { align_button("Align left", "⊢", AlignMode::Left) }
                { align_button("Align horizontal center", "⋮", AlignMode::HCenter) }
                { align_button("Align right", "⊣", AlignMode::Right) }
                { align_button("Align top", "⊤", AlignMode::Top) }
                { align_button("Align vertical center", "⋯", AlignMode::VCenter) }
                { align_button("Align bottom", "⊥", AlignMode::Bottom) }
            </div>
            <h3 class="right-panel__subheading right-panel__subheading--spaced">{"Distribute"}</h3>
            <div class="right-panel__align-row">
                { distribute_button("Distribute horizontally", "⇔ H", DistributeAxis::Horizontal) }
                { distribute_button("Distribute vertically", "⇕ V", DistributeAxis::Vertical) }
            </div>
            { if count < 3 {
                html! { <div class="right-panel__align-hint">{"Select 3+ objects to distribute"}</div> }
            } else {
                html! {}
            } }
        </div>
    }
}

#[function_component(RightPanel)]
pub fn right_panel() -> Html {
    let editor = use_editor();
    let dragged = use_state(|| None::<ObjectId>);
    let replace_image_input_ref = use_node_ref();

    // A group's own `geometry` is just its bounding box at creation time,
    // never kept live — never show it as if it were an editable object.
    let selected = (editor.selected_ids.len() == 1)
        .then(|| *editor.selected_ids.iter().next().unwrap())
        .and_then(|id| editor.document.get(id))
        .filter(|object| !object.kind.is_group())
        .map(|object| (object.id, object.geometry));

    let selected_text: Option<(ObjectId, TextProperties)> = (editor.selected_ids.len() == 1)
        .then(|| *editor.selected_ids.iter().next().unwrap())
        .and_then(|id| editor.document.get(id))
        .and_then(|object| match &object.kind {
            ObjectKind::Text(props) => Some((object.id, props.clone())),
            _ => None,
        });

    let selected_image: Option<(ObjectId, ImageProperties)> = (editor.selected_ids.len() == 1)
        .then(|| *editor.selected_ids.iter().next().unwrap())
        .and_then(|id| editor.document.get(id))
        .and_then(|object| match &object.kind {
            ObjectKind::Image(props) => Some((object.id, *props)),
            _ => None,
        });

    let layer_rows = render_layer_rows(&editor.document, &editor, &dragged, None, 0);

    // A single text selection takes over the whole sidebar — Layers makes
    // way for its properties rather than sitting above them — and Layers
    // comes back the moment the selection moves off text.
    let text_selected = selected_text.is_some();
    let properties_class = classes!(
        "right-panel__section",
        "right-panel__section--properties",
        text_selected.then_some("right-panel__section--properties--full")
    );

    html! {
        <aside class="right-panel">
            { if !text_selected {
                html! {
                    <div class="right-panel__section right-panel__section--layers">
                        <h2 class="right-panel__heading">{"Layers"}</h2>
                        { if layer_rows.is_empty() {
                            render_empty_state("No objects yet", "Create a shape, text, or image to begin.")
                        } else {
                            html! { <ul class="right-panel__layers">{ for layer_rows }</ul> }
                        } }
                    </div>
                }
            } else {
                html! {}
            } }

            <div class={properties_class}>
                <h2 class="right-panel__heading">{"Properties"}</h2>
                { render_inspector(&editor, &selected, &selected_text, &selected_image, replace_image_input_ref) }
            </div>
        </aside>
    }
}

/// Picks the one inspector that matches the current selection — the sole
/// place that decides Empty vs. Text vs. Image vs. Shape vs.
/// MultiSelection, so nothing below Layers needs to re-derive that itself.
/// Keyed by kind so a selection-type change replaces the subtree (rather
/// than patching it in place), which is what lets the subtle fade below
/// apply per-switch instead of once on first mount.
fn render_inspector(
    editor: &EditorContext,
    selected: &Option<(ObjectId, Geometry)>,
    selected_text: &Option<(ObjectId, TextProperties)>,
    selected_image: &Option<(ObjectId, ImageProperties)>,
    replace_image_input_ref: NodeRef,
) -> Html {
    let (kind, content) = if editor.selected_ids.len() >= 2 {
        ("multi", render_multi_selection_inspector(editor))
    } else if selected_text.is_some() {
        ("text", render_text_inspector(editor, selected_text, *selected))
    } else if selected_image.is_some() {
        ("image", render_image_inspector(editor, &editor.document, selected_image, *selected, replace_image_input_ref))
    } else if selected.is_some() {
        ("shape", render_shape_inspector(editor, *selected))
    } else {
        ("empty", render_empty_inspector())
    };

    html! {
        <div key={kind} class="right-panel__inspector">
            { content }
        </div>
    }
}
