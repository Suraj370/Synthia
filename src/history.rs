//! Undo/redo as a command stack. Each `Command` stores only the small typed
//! data it needs to apply or invert itself (an object's fields, a before/
//! after `Geometry`, an index) — never rendered output, so this stays cheap
//! regardless of how complex the document gets.

use crate::model::{DesignDocument, DesignObject, Geometry, ImageProperties, LineProperties, ObjectId, ObjectKind, PathProperties, ShapeProperties, TextProperties};

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// A single object was created (draw tool, paste, duplicate, group).
    CreateObject { object: DesignObject },
    /// One or more objects were removed, each with the index it occupied so
    /// undo can restore the exact original z-order.
    DeleteObjects { removed: Vec<(usize, DesignObject)> },
    /// An object's geometry changed (move, resize, rotate, or a Properties
    /// field edit — all of them already flow through the same `Geometry`).
    SetGeometry { id: ObjectId, before: Geometry, after: Geometry },
    /// A text object's typography/content properties changed — one whole
    /// edit session (typing, Ctrl+B/I toggles) collapsed into a single
    /// undo step, or one Properties panel field edit. Geometry side
    /// effects (auto-size, or a resize that flips Auto to Fixed) are their
    /// own separate `SetGeometry`, batched alongside this when both change
    /// together, so this command only ever owns the text fields.
    SetTextProperties { id: ObjectId, before: TextProperties, after: TextProperties },
    /// An image object's fit mode or asset reference changed (a Properties
    /// panel edit, or "Replace image" — both just swap `ImageProperties`).
    SetImageProperties { id: ObjectId, before: ImageProperties, after: ImageProperties },
    /// A rectangle/ellipse's fill/stroke changed (a Properties panel field,
    /// or one drag gesture on the color picker collapsed into a single
    /// step).
    SetShapeProperties { id: ObjectId, before: ShapeProperties, after: ShapeProperties },
    /// A line's endpoints or stroke color/width changed (endpoint drags are
    /// `SetGeometry` since the box IS the endpoints — see `model.rs`'s
    /// `PathPoint` docs; this only covers Properties panel / color-picker
    /// edits to the stroke).
    SetLineProperties { id: ObjectId, before: LineProperties, after: LineProperties },
    /// A freehand path's stroke color/width changed. The recorded points
    /// themselves never change after creation — only a new drawing can
    /// produce different points.
    SetPathProperties { id: ObjectId, before: PathProperties, after: PathProperties },
    /// One or more objects were reparented (grouping/ungrouping).
    SetParent { changes: Vec<(ObjectId, Option<ObjectId>, Option<ObjectId>)> },
    /// An object moved to a different position in document (z-)order, e.g.
    /// via drag-reordering in the Layers panel.
    Reorder { id: ObjectId, before_index: usize, after_index: usize },
    /// The document canvas width/height changed (Canvas Size panel, or a
    /// preset). When "Scale content" was on, the object geometry changes
    /// that came along with it are separate `SetGeometry` commands batched
    /// alongside this one, not folded into it.
    SetCanvasSize { before: (f64, f64), after: (f64, f64) },
    /// The document background color changed (Canvas Size panel).
    SetBackground { before: String, after: String },
    /// Several commands that only make sense as one undo/redo step.
    Batch(Vec<Command>),
}

impl Command {
    pub fn apply(&self, document: &mut DesignDocument) {
        match self {
            Command::CreateObject { object } => document.insert_object(object.clone()),
            Command::DeleteObjects { removed } => {
                for (_, object) in removed {
                    document.remove(object.id);
                }
            }
            Command::SetGeometry { id, after, .. } => {
                if let Some(object) = document.get_mut(*id) {
                    object.geometry = *after;
                }
            }
            Command::SetTextProperties { id, after, .. } => {
                if let Some(object) = document.get_mut(*id) {
                    object.kind = ObjectKind::Text(after.clone());
                }
            }
            Command::SetImageProperties { id, after, .. } => {
                if let Some(object) = document.get_mut(*id) {
                    object.kind = ObjectKind::Image(*after);
                }
            }
            Command::SetShapeProperties { id, after, .. } => {
                if let Some(object) = document.get_mut(*id) {
                    match &mut object.kind {
                        ObjectKind::Rectangle(props) | ObjectKind::Ellipse(props) => *props = *after,
                        _ => {}
                    }
                }
            }
            Command::SetLineProperties { id, after, .. } => {
                if let Some(object) = document.get_mut(*id) {
                    object.kind = ObjectKind::Line(*after);
                }
            }
            Command::SetPathProperties { id, after, .. } => {
                if let Some(object) = document.get_mut(*id) {
                    object.kind = ObjectKind::Path(after.clone());
                }
            }
            Command::SetParent { changes } => {
                for (id, _, after) in changes {
                    if let Some(object) = document.get_mut(*id) {
                        object.parent_id = *after;
                    }
                }
            }
            Command::Reorder { id, after_index, .. } => {
                document.move_to_index(*id, *after_index);
            }
            Command::SetCanvasSize { after, .. } => {
                document.canvas_width = after.0;
                document.canvas_height = after.1;
            }
            Command::SetBackground { after, .. } => {
                document.background = after.clone();
            }
            Command::Batch(commands) => {
                for command in commands {
                    command.apply(document);
                }
            }
        }
    }

    pub fn unapply(&self, document: &mut DesignDocument) {
        match self {
            Command::CreateObject { object } => {
                document.remove(object.id);
            }
            Command::DeleteObjects { removed } => {
                for (index, object) in removed {
                    document.insert_at(*index, object.clone());
                }
            }
            Command::SetGeometry { id, before, .. } => {
                if let Some(object) = document.get_mut(*id) {
                    object.geometry = *before;
                }
            }
            Command::SetTextProperties { id, before, .. } => {
                if let Some(object) = document.get_mut(*id) {
                    object.kind = ObjectKind::Text(before.clone());
                }
            }
            Command::SetImageProperties { id, before, .. } => {
                if let Some(object) = document.get_mut(*id) {
                    object.kind = ObjectKind::Image(*before);
                }
            }
            Command::SetShapeProperties { id, before, .. } => {
                if let Some(object) = document.get_mut(*id) {
                    match &mut object.kind {
                        ObjectKind::Rectangle(props) | ObjectKind::Ellipse(props) => *props = *before,
                        _ => {}
                    }
                }
            }
            Command::SetLineProperties { id, before, .. } => {
                if let Some(object) = document.get_mut(*id) {
                    object.kind = ObjectKind::Line(*before);
                }
            }
            Command::SetPathProperties { id, before, .. } => {
                if let Some(object) = document.get_mut(*id) {
                    object.kind = ObjectKind::Path(before.clone());
                }
            }
            Command::SetParent { changes } => {
                for (id, before, _) in changes {
                    if let Some(object) = document.get_mut(*id) {
                        object.parent_id = *before;
                    }
                }
            }
            Command::Reorder { id, before_index, .. } => {
                document.move_to_index(*id, *before_index);
            }
            Command::SetCanvasSize { before, .. } => {
                document.canvas_width = before.0;
                document.canvas_height = before.1;
            }
            Command::SetBackground { before, .. } => {
                document.background = before.clone();
            }
            Command::Batch(commands) => {
                for command in commands.iter().rev() {
                    command.unapply(document);
                }
            }
        }
    }

    /// Ids this command touches, so the caller can update selection to
    /// follow the undo/redo instead of leaving a stale selection behind.
    pub fn affected_ids(&self) -> Vec<ObjectId> {
        match self {
            Command::CreateObject { object } => vec![object.id],
            Command::DeleteObjects { removed } => removed.iter().map(|(_, o)| o.id).collect(),
            Command::SetGeometry { id, .. } => vec![*id],
            Command::SetTextProperties { id, .. } => vec![*id],
            Command::SetImageProperties { id, .. } => vec![*id],
            Command::SetShapeProperties { id, .. } => vec![*id],
            Command::SetLineProperties { id, .. } => vec![*id],
            Command::SetPathProperties { id, .. } => vec![*id],
            Command::SetParent { changes } => changes.iter().map(|(id, ..)| *id).collect(),
            Command::Reorder { id, .. } => vec![*id],
            Command::SetCanvasSize { .. } => Vec::new(),
            Command::SetBackground { .. } => Vec::new(),
            Command::Batch(commands) => commands.iter().flat_map(Command::affected_ids).collect(),
        }
    }
}

/// A single non-trivial `Command` collapses to itself instead of a
/// one-element batch, keeping the common case (one move, one create) simple.
pub fn batch(commands: Vec<Command>) -> Option<Command> {
    match commands.len() {
        0 => None,
        1 => commands.into_iter().next(),
        _ => Some(Command::Batch(commands)),
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct History {
    undo_stack: Vec<Command>,
    redo_stack: Vec<Command>,
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Changes on every `record`, `undo`, and `redo` (each pushes/pops
    /// `undo_stack` by exactly one) — a cheap, exact signal that the
    /// document was just permanently changed, used to mark the document
    /// dirty without diffing it. Two dispatches with the same count mean
    /// nothing was committed between them (a live/uncommitted preview
    /// update, or an editor-only action like changing selection or zoom).
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn record(&mut self, command: Command) {
        self.undo_stack.push(command);
        self.redo_stack.clear();
    }

    pub fn undo(&mut self, document: &mut DesignDocument) -> Option<Vec<ObjectId>> {
        let command = self.undo_stack.pop()?;
        command.unapply(document);
        let ids = command.affected_ids();
        self.redo_stack.push(command);
        Some(ids)
    }

    pub fn redo(&mut self, document: &mut DesignDocument) -> Option<Vec<ObjectId>> {
        let command = self.redo_stack.pop()?;
        command.apply(document);
        let ids = command.affected_ids();
        self.undo_stack.push(command);
        Some(ids)
    }
}
