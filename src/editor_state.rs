//! Editor state: the active tool, current selection, clipboard, Layers
//! panel expansion, and undo/redo history — everything the UI needs beyond
//! the document itself. Kept separate from both the document model
//! (`model.rs`) and the UI components, which only read this state and
//! dispatch `EditorAction`s.

use std::collections::BTreeSet;
use std::rc::Rc;
use yew::prelude::*;

use crate::history::{batch, Command, History};
use crate::model::{DesignDocument, DesignObject, Geometry, ObjectId, ObjectKind, TextProperties, TextSizeMode, DUPLICATE_OFFSET};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignMode {
    Left,
    HCenter,
    Right,
    Top,
    VCenter,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistributeAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, PartialEq, Default)]
pub struct EditorState {
    pub document: DesignDocument,
    pub selected_ids: BTreeSet<ObjectId>,
    pub active_tool: Tool,
    pub history: History,
    pub clipboard: Vec<DesignObject>,
    /// Which `Group` objects are expanded in the Layers panel. Purely a UI
    /// concern — the document doesn't know about it, and it's never part
    /// of undo/redo history.
    pub expanded_groups: BTreeSet<ObjectId>,
    /// The text object currently in inline edit mode on the canvas, if
    /// any. Purely a UI-mode flag — never recorded to history itself; the
    /// content/property changes made during the session are what get
    /// recorded, as one step, when it ends.
    pub editing_text: Option<ObjectId>,
}

impl EditorState {
    pub fn is_selected(&self, id: ObjectId) -> bool {
        self.selected_ids.contains(&id)
    }
}

pub enum EditorAction {
    SetActiveTool(Tool),
    CreateObject { kind: ObjectKind, geometry: Geometry },
    SetSelection(BTreeSet<ObjectId>),
    /// Live geometry update during an active drag — applied to the
    /// document immediately (so the canvas and Properties panel track the
    /// pointer) but NOT recorded to history. A drag records history once,
    /// via `CommitGeometries`, when it ends.
    UpdateGeometry { id: ObjectId, geometry: Geometry },
    /// One undoable step covering one or more objects' geometry changing
    /// from `before` to `after` (a finished move/resize drag, a Properties
    /// field edit, or an arrow-key nudge).
    CommitGeometries(Vec<(ObjectId, Geometry, Geometry)>),
    DeleteObjects(Vec<ObjectId>),
    DuplicateObjects(Vec<ObjectId>),
    Copy,
    Paste,
    GroupSelected,
    UngroupSelected,
    ToggleGroupExpanded(ObjectId),
    ReorderObject { id: ObjectId, new_index: usize },
    /// Moves every selected object/group so its bounding box aligns with
    /// the overall selection bounding box along `mode`'s axis.
    AlignObjects(AlignMode),
    /// Spaces 3+ selected objects/groups evenly along `axis`, keeping the
    /// first and last (by position) fixed.
    DistributeObjects(DistributeAxis),
    /// Enters (`Some`) or exits (`None`) inline text-edit mode for an
    /// object, without touching history. Exiting via this action alone
    /// (rather than `CommitTextEdit`) discards nothing — the document is
    /// already the live source of truth for whatever was typed.
    SetEditingText(Option<ObjectId>),
    /// Live text-property update during an active edit session (typing, or
    /// a Ctrl+B/I toggle) — applied to the document immediately but NOT
    /// recorded to history, the same way `UpdateGeometry` works for drags.
    UpdateTextProperties { id: ObjectId, properties: TextProperties },
    /// Ends an edit session: records one undo step covering everything
    /// that changed since it started (reading the *current*, already-live
    /// document as the "after" state), and exits edit mode. A session that
    /// created empty text and never got any content is deleted outright
    /// instead of leaving an invisible object behind.
    CommitTextEdit { id: ObjectId, before: TextProperties, before_geometry: Geometry },
    /// One immediate, undoable text-property change from a Properties
    /// panel field (not part of an active edit session).
    CommitTextProperties { id: ObjectId, before: TextProperties, after: TextProperties },
    /// Finishes a resize-handle drag on a text object: records the
    /// geometry change plus, for `Auto`-size text, the implied switch to
    /// `Fixed` mode (dragging a handle is the only way to reach `Fixed`),
    /// as one undo step.
    CommitTextResize { id: ObjectId, before_geometry: Geometry, before_props: TextProperties, after_props: TextProperties },
    Undo,
    Redo,
}

/// Writes `properties` onto `id`'s `ObjectKind::Text`, and — for
/// auto-size text — recomputes width/height to match the new content/font
/// settings. Shared by every text-editing action so auto-size stays
/// consistent regardless of whether the change came from typing, a
/// Ctrl+B/I toggle, or a Properties panel field.
fn write_text_properties(document: &mut DesignDocument, id: ObjectId, properties: TextProperties) {
    let Some(object) = document.get_mut(id) else { return };
    let size_mode = properties.size_mode;
    object.kind = ObjectKind::Text(properties.clone());
    if size_mode == TextSizeMode::Auto {
        let (width, height) = crate::text_metrics::auto_size(&properties);
        object.geometry.width = width;
        object.geometry.height = height;
    }
}

/// The non-group leaf ids "under" `id` for movement purposes: itself if
/// it's not a group, or every non-group descendant if it is. Mirrors
/// `CanvasArea::move_targets` (a group has no shape of its own, so aligning
/// or distributing it has to move its members without disturbing their
/// layout relative to each other).
fn leaf_ids_of(document: &DesignDocument, id: ObjectId) -> Vec<ObjectId> {
    if document.get(id).map(|o| o.kind.is_group()).unwrap_or(false) {
        document
            .descendants_of(id)
            .into_iter()
            .filter(|descendant| !document.get(*descendant).map(|o| o.kind.is_group()).unwrap_or(true))
            .collect()
    } else {
        vec![id]
    }
}

/// Each selected top-level id resolved to its leaf ids plus the live
/// bounding box those leaves currently occupy.
fn alignment_units(document: &DesignDocument, selected: &BTreeSet<ObjectId>) -> Vec<(Vec<ObjectId>, Geometry)> {
    selected
        .iter()
        .filter_map(|&id| {
            let leaves = leaf_ids_of(document, id);
            document.bounding_box(&leaves).map(|bbox| (leaves, bbox))
        })
        .collect()
}

/// The (dx, dy) that moves `bbox` so it aligns with `reference` per `mode`.
fn alignment_delta(mode: AlignMode, reference: &Geometry, bbox: &Geometry) -> (f64, f64) {
    match mode {
        AlignMode::Left => (reference.x - bbox.x, 0.0),
        AlignMode::HCenter => (reference.x + reference.width / 2.0 - (bbox.x + bbox.width / 2.0), 0.0),
        AlignMode::Right => (reference.x + reference.width - (bbox.x + bbox.width), 0.0),
        AlignMode::Top => (0.0, reference.y - bbox.y),
        AlignMode::VCenter => (0.0, reference.y + reference.height / 2.0 - (bbox.y + bbox.height / 2.0)),
        AlignMode::Bottom => (0.0, reference.y + reference.height - (bbox.y + bbox.height)),
    }
}

/// Builds the geometry changes for nudging every leaf in `leaves` by
/// `(dx, dy)`, reading `before` from `document` (not yet mutated).
fn shift_changes(document: &DesignDocument, leaves: &[ObjectId], dx: f64, dy: f64) -> Vec<(ObjectId, Geometry, Geometry)> {
    if dx == 0.0 && dy == 0.0 {
        return Vec::new();
    }
    leaves
        .iter()
        .filter_map(|&leaf| {
            document.get(leaf).map(|object| {
                let before = object.geometry;
                (leaf, before, Geometry { x: before.x + dx, y: before.y + dy, ..before })
            })
        })
        .collect()
}

/// Applies a batch of already-computed geometry changes to `document` and
/// records one undoable command for them, if any.
fn apply_and_record(document: &mut DesignDocument, history: &mut History, changes: Vec<(ObjectId, Geometry, Geometry)>) {
    for (id, _, after) in &changes {
        if let Some(object) = document.get_mut(*id) {
            object.geometry = *after;
        }
    }
    if let Some(command) = batch(changes.into_iter().map(|(id, before, after)| Command::SetGeometry { id, before, after }).collect()) {
        history.record(command);
    }
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
                let is_text = matches!(kind, ObjectKind::Text(_));
                let id = next.document.insert(kind, geometry);
                if let Some(object) = next.document.get(id) {
                    next.history.record(Command::CreateObject { object: object.clone() });
                }
                next.selected_ids = BTreeSet::from([id]);
                next.active_tool = Tool::Select;
                // New text drops straight into edit mode so the user can
                // start typing immediately, instead of a click-then-click.
                next.editing_text = if is_text { Some(id) } else { None };
            }

            EditorAction::SetSelection(ids) => {
                next.selected_ids = ids;
            }

            EditorAction::UpdateGeometry { id, geometry } => {
                if let Some(object) = next.document.get_mut(id) {
                    object.geometry = geometry;
                }
            }

            EditorAction::CommitGeometries(changes) => {
                let mut commands = Vec::with_capacity(changes.len());
                for (id, before, after) in changes {
                    if let Some(object) = next.document.get_mut(id) {
                        object.geometry = after;
                    }
                    commands.push(Command::SetGeometry { id, before, after });
                }
                if let Some(command) = batch(commands) {
                    next.history.record(command);
                }
            }

            EditorAction::DeleteObjects(ids) => {
                let all_ids = expand_with_descendants(&next.document, &ids);
                let mut removed: Vec<(usize, DesignObject)> = all_ids
                    .iter()
                    .filter_map(|id| {
                        let index = next.document.objects.iter().position(|o| o.id == *id)?;
                        Some((index, next.document.objects[index].clone()))
                    })
                    .collect();
                removed.sort_by_key(|(index, _)| *index);
                for (_, object) in &removed {
                    next.document.remove(object.id);
                }
                for id in &all_ids {
                    next.selected_ids.remove(id);
                }
                if !removed.is_empty() {
                    next.history.record(Command::DeleteObjects { removed });
                }
            }

            EditorAction::DuplicateObjects(ids) => {
                let all_ids = expand_with_descendants(&next.document, &ids);
                let sources: Vec<DesignObject> = all_ids.iter().filter_map(|id| next.document.get(*id).cloned()).collect();
                let created = next.document.insert_many_remapped(&sources, DUPLICATE_OFFSET, DUPLICATE_OFFSET);
                next.selected_ids = created.iter().map(|o| o.id).collect();
                if let Some(command) = batch(created.into_iter().map(|object| Command::CreateObject { object }).collect()) {
                    next.history.record(command);
                }
            }

            EditorAction::Copy => {
                let all_ids = expand_with_descendants(&next.document, &next.selected_ids.iter().copied().collect::<Vec<_>>());
                next.clipboard = all_ids.iter().filter_map(|id| next.document.get(*id).cloned()).collect();
            }

            EditorAction::Paste => {
                if !next.clipboard.is_empty() {
                    let sources = next.clipboard.clone();
                    let created = next.document.insert_many_remapped(&sources, DUPLICATE_OFFSET, DUPLICATE_OFFSET);
                    next.selected_ids = created.iter().map(|o| o.id).collect();
                    if let Some(command) = batch(created.into_iter().map(|object| Command::CreateObject { object }).collect()) {
                        next.history.record(command);
                    }
                }
            }

            EditorAction::GroupSelected => {
                if next.selected_ids.len() >= 2 {
                    let ids: Vec<ObjectId> = next.selected_ids.iter().copied().collect();
                    if let Some((group_object, changes)) = next.document.group(&ids) {
                        next.expanded_groups.insert(group_object.id);
                        next.selected_ids = BTreeSet::from([group_object.id]);
                        next.history.record(Command::Batch(vec![
                            Command::CreateObject { object: group_object },
                            Command::SetParent { changes },
                        ]));
                    }
                }
            }

            EditorAction::UngroupSelected => {
                let group_ids: Vec<ObjectId> = next
                    .selected_ids
                    .iter()
                    .copied()
                    .filter(|id| next.document.get(*id).map(|o| o.kind.is_group()).unwrap_or(false))
                    .collect();

                if !group_ids.is_empty() {
                    let mut all_changes = Vec::new();
                    let mut removed = Vec::new();
                    let mut new_selection = BTreeSet::new();

                    for group_id in group_ids {
                        let grandparent = next.document.get(group_id).and_then(|o| o.parent_id);
                        let children: Vec<ObjectId> = next.document.children_of(Some(group_id)).map(|o| o.id).collect();
                        for child in children {
                            if let Some(object) = next.document.get_mut(child) {
                                let before = object.parent_id;
                                object.parent_id = grandparent;
                                all_changes.push((child, before, grandparent));
                                new_selection.insert(child);
                            }
                        }
                        next.expanded_groups.remove(&group_id);
                        if let Some((index, object)) = next.document.remove_with_index(group_id) {
                            removed.push((index, object));
                        }
                    }

                    next.selected_ids = new_selection;
                    next.history.record(Command::Batch(vec![
                        Command::SetParent { changes: all_changes },
                        Command::DeleteObjects { removed },
                    ]));
                }
            }

            EditorAction::ToggleGroupExpanded(id) => {
                if !next.expanded_groups.remove(&id) {
                    next.expanded_groups.insert(id);
                }
            }

            EditorAction::ReorderObject { id, new_index } => {
                if let Some(before_index) = next.document.move_to_index(id, new_index) {
                    next.history.record(Command::Reorder { id, before_index, after_index: new_index });
                }
            }

            EditorAction::AlignObjects(mode) => {
                let units = alignment_units(&next.document, &next.selected_ids);
                if units.len() >= 2 {
                    let all_leaves: Vec<ObjectId> = units.iter().flat_map(|(leaves, _)| leaves.iter().copied()).collect();
                    if let Some(overall) = next.document.bounding_box(&all_leaves) {
                        let mut changes = Vec::new();
                        for (leaves, bbox) in &units {
                            let (dx, dy) = alignment_delta(mode, &overall, bbox);
                            changes.extend(shift_changes(&next.document, leaves, dx, dy));
                        }
                        apply_and_record(&mut next.document, &mut next.history, changes);
                    }
                }
            }

            EditorAction::DistributeObjects(axis) => {
                let mut units = alignment_units(&next.document, &next.selected_ids);
                if units.len() >= 3 {
                    match axis {
                        DistributeAxis::Horizontal => units.sort_by(|a, b| a.1.x.partial_cmp(&b.1.x).unwrap()),
                        DistributeAxis::Vertical => units.sort_by(|a, b| a.1.y.partial_cmp(&b.1.y).unwrap()),
                    }

                    let first_bbox = units[0].1;
                    let last_bbox = units[units.len() - 1].1;
                    let gap_count = (units.len() - 1) as f64;
                    let middle = &units[1..units.len() - 1];

                    let mut changes = Vec::new();
                    match axis {
                        DistributeAxis::Horizontal => {
                            let span_start = first_bbox.x + first_bbox.width;
                            let span_end = last_bbox.x;
                            let middle_widths: f64 = middle.iter().map(|(_, bbox)| bbox.width).sum();
                            let gap = (span_end - span_start - middle_widths) / gap_count;
                            let mut cursor = span_start;
                            for (leaves, bbox) in middle {
                                let new_x = cursor + gap;
                                changes.extend(shift_changes(&next.document, leaves, new_x - bbox.x, 0.0));
                                cursor = new_x + bbox.width;
                            }
                        }
                        DistributeAxis::Vertical => {
                            let span_start = first_bbox.y + first_bbox.height;
                            let span_end = last_bbox.y;
                            let middle_heights: f64 = middle.iter().map(|(_, bbox)| bbox.height).sum();
                            let gap = (span_end - span_start - middle_heights) / gap_count;
                            let mut cursor = span_start;
                            for (leaves, bbox) in middle {
                                let new_y = cursor + gap;
                                changes.extend(shift_changes(&next.document, leaves, 0.0, new_y - bbox.y));
                                cursor = new_y + bbox.height;
                            }
                        }
                    }
                    apply_and_record(&mut next.document, &mut next.history, changes);
                }
            }

            EditorAction::SetEditingText(id) => {
                next.editing_text = id;
            }

            EditorAction::UpdateTextProperties { id, properties } => {
                write_text_properties(&mut next.document, id, properties);
            }

            EditorAction::CommitTextEdit { id, before, before_geometry } => {
                next.editing_text = None;
                let after = next.document.get(id).and_then(|object| match &object.kind {
                    ObjectKind::Text(props) => Some(props.clone()),
                    _ => None,
                });
                match after {
                    // A session that created text and got no content out
                    // of it: remove the object instead of leaving an
                    // invisible one behind.
                    Some(after) if before.content.trim().is_empty() && after.content.trim().is_empty() => {
                        if let Some((index, object)) = next.document.remove_with_index(id) {
                            next.selected_ids.remove(&id);
                            next.history.record(Command::DeleteObjects { removed: vec![(index, object)] });
                        }
                    }
                    Some(after) => {
                        let after_geometry = next.document.get(id).map(|o| o.geometry).unwrap_or(before_geometry);
                        let mut commands = Vec::new();
                        if before != after {
                            commands.push(Command::SetTextProperties { id, before, after });
                        }
                        if before_geometry != after_geometry {
                            commands.push(Command::SetGeometry { id, before: before_geometry, after: after_geometry });
                        }
                        if let Some(command) = batch(commands) {
                            next.history.record(command);
                        }
                    }
                    None => {}
                }
            }

            EditorAction::CommitTextProperties { id, before, after } => {
                if let Some(object) = next.document.get(id) {
                    let before_geometry = object.geometry;
                    write_text_properties(&mut next.document, id, after.clone());
                    let after_geometry = next.document.get(id).map(|o| o.geometry).unwrap_or(before_geometry);
                    let mut commands = Vec::new();
                    if before != after {
                        commands.push(Command::SetTextProperties { id, before, after });
                    }
                    if before_geometry != after_geometry {
                        commands.push(Command::SetGeometry { id, before: before_geometry, after: after_geometry });
                    }
                    if let Some(command) = batch(commands) {
                        next.history.record(command);
                    }
                }
            }

            EditorAction::CommitTextResize { id, before_geometry, before_props, after_props } => {
                if let Some(object) = next.document.get_mut(id) {
                    let after_geometry = object.geometry;
                    object.kind = ObjectKind::Text(after_props.clone());
                    let mut commands = Vec::new();
                    if before_props != after_props {
                        commands.push(Command::SetTextProperties { id, before: before_props, after: after_props });
                    }
                    if before_geometry != after_geometry {
                        commands.push(Command::SetGeometry { id, before: before_geometry, after: after_geometry });
                    }
                    if let Some(command) = batch(commands) {
                        next.history.record(command);
                    }
                }
            }

            EditorAction::Undo => {
                if let Some(ids) = next.history.undo(&mut next.document) {
                    next.selected_ids = ids.into_iter().filter(|id| next.document.get(*id).is_some()).collect();
                }
            }

            EditorAction::Redo => {
                if let Some(ids) = next.history.redo(&mut next.document) {
                    next.selected_ids = ids.into_iter().filter(|id| next.document.get(*id).is_some()).collect();
                }
            }
        }
        Rc::new(next)
    }
}

/// `ids` plus every descendant of any of them that is a group — the set an
/// operation on a group (delete, duplicate, copy) needs to actually cover.
fn expand_with_descendants(document: &DesignDocument, ids: &[ObjectId]) -> Vec<ObjectId> {
    let mut all_ids = Vec::new();
    for &id in ids {
        all_ids.push(id);
        all_ids.extend(document.descendants_of(id));
    }
    all_ids.sort_unstable();
    all_ids.dedup();
    all_ids
}

pub type EditorContext = UseReducerHandle<EditorState>;

/// Reads the shared editor state provided by `App`.
#[hook]
pub fn use_editor() -> EditorContext {
    use_context::<EditorContext>().expect("EditorContext to be provided by App")
}
