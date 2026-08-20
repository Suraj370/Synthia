//! Editor state: the active tool, current selection, and the document
//! itself. Kept separate from both the document model (`model.rs`) and the
//! UI components, which only read this state and dispatch `EditorAction`s.

use std::rc::Rc;
use yew::prelude::*;

use crate::model::{DesignDocument, Geometry, ObjectId, ObjectKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Rectangle,
    Ellipse,
    Line,
    Pen,
    Text,
    Hand,
}

#[derive(Clone, PartialEq, Default)]
pub struct EditorState {
    pub document: DesignDocument,
    pub selected_id: Option<ObjectId>,
    pub active_tool: Tool,
}

pub enum EditorAction {
    SetActiveTool(Tool),
    CreateObject { kind: ObjectKind, geometry: Geometry },
    SelectObject(Option<ObjectId>),
}

impl Reducible for EditorState {
    type Action = EditorAction;

    fn reduce(self: Rc<Self>, action: Self::Action) -> Rc<Self> {
        let mut next = (*self).clone();
        match action {
            EditorAction::SetActiveTool(tool) => {
                next.active_tool = tool;
            }
            EditorAction::CreateObject { kind, geometry } => {
                let id = next.document.insert(kind, geometry);
                next.selected_id = Some(id);
                next.active_tool = Tool::Select;
            }
            EditorAction::SelectObject(id) => {
                next.selected_id = id;
            }
        }
        Rc::new(next)
    }
}

pub type EditorContext = UseReducerHandle<EditorState>;

/// Reads the shared editor state provided by `App`.
#[hook]
pub fn use_editor() -> EditorContext {
    use_context::<EditorContext>().expect("EditorContext to be provided by App")
}
