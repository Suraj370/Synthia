//! Undo/redo as a command stack. Each `Command` stores only the small typed
//! data it needs to apply or invert itself (an object's fields, a before/
//! after `Geometry`, an index) — never rendered output, so this stays cheap
//! regardless of how complex the document gets.

use crate::model::{DesignDocument, DesignObject, Geometry, ObjectId, ObjectKind, TextProperties};

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
    /// One or more objects were reparented (grouping/ungrouping).
    SetParent { changes: Vec<(ObjectId, Option<ObjectId>, Option<ObjectId>)> },
    /// An object moved to a different position in document (z-)order, e.g.
    /// via drag-reordering in the Layers panel.
    Reorder { id: ObjectId, before_index: usize, after_index: usize },
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
            Command::SetParent { changes } => changes.iter().map(|(id, ..)| *id).collect(),
            Command::Reorder { id, .. } => vec![*id],
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
