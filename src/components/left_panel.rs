use yew::prelude::*;

use crate::editor_state::{use_editor, EditorAction, Tool};

struct ToolDef {
    tool: Tool,
    label: &'static str,
    icon: &'static str,
}

const fn def(tool: Tool, label: &'static str, icon: &'static str) -> ToolDef {
    ToolDef { tool, label, icon }
}

/// Tools grouped by function — selection, shapes, text, and view — with a
/// thin divider rendered between groups.
const TOOL_GROUPS: &[&[ToolDef]] = &[
    &[def(Tool::Select, "Select", "⟁")],
    &[
        def(Tool::Rectangle, "Rectangle", "▭"),
        def(Tool::Ellipse, "Ellipse", "○"),
        def(Tool::Line, "Line", "╱"),
        def(Tool::Pen, "Pen", "✎"),
    ],
    &[def(Tool::Text, "Text", "T")],
    &[def(Tool::Hand, "Pan", "✋︎")],
];

#[function_component(LeftPanel)]
pub fn left_panel() -> Html {
    let editor = use_editor();

    html! {
        <aside class="left-panel">
            { for TOOL_GROUPS.iter().enumerate().map(|(group_index, group)| html! {
                <>
                    { if group_index > 0 { html! { <div class="left-panel__divider"></div> } } else { html! {} } }
                    <div class="left-panel__group">
                        { for group.iter().map(|def| {
                            let class = if editor.active_tool == def.tool {
                                "left-panel__tool left-panel__tool--active"
                            } else {
                                "left-panel__tool"
                            };
                            let editor = editor.clone();
                            let tool = def.tool;
                            let onclick = Callback::from(move |_| editor.dispatch(EditorAction::SetActiveTool(tool)));

                            html! {
                                <button key={def.label} {class} title={def.label} {onclick}>
                                    <span class="left-panel__icon">{def.icon}</span>
                                </button>
                            }
                        }) }
                    </div>
                </>
            }) }
        </aside>
    }
}
