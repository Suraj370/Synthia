use yew::prelude::*;

use crate::editor_state::{use_editor, EditorAction, Tool};

struct ToolDef {
    tool: Tool,
    label: &'static str,
    icon: &'static str,
}

const TOOLS: &[ToolDef] = &[
    ToolDef { tool: Tool::Select, label: "Select", icon: "⟁" },
    ToolDef { tool: Tool::Rectangle, label: "Rectangle", icon: "▭" },
    ToolDef { tool: Tool::Ellipse, label: "Ellipse", icon: "○" },
    ToolDef { tool: Tool::Line, label: "Line", icon: "╱" },
    ToolDef { tool: Tool::Pen, label: "Pen", icon: "✎" },
    ToolDef { tool: Tool::Text, label: "Text", icon: "T" },
    ToolDef { tool: Tool::Hand, label: "Pan", icon: "✋︎" },
];

#[function_component(LeftPanel)]
pub fn left_panel() -> Html {
    let editor = use_editor();

    html! {
        <aside class="left-panel">
            { for TOOLS.iter().map(|def| {
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
        </aside>
    }
}
