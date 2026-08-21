//! Editor state: the active tool, current selection, clipboard, Layers
//! panel expansion, and undo/redo history — everything the UI needs beyond
//! the document itself. Kept separate from both the document model
//! (`model.rs`) and the UI components, which only read this state and
//! dispatch `EditorAction`s.

use std::collections::BTreeSet;
use std::rc::Rc;
use yew::prelude::*;

use crate::history::{batch, Command, History};
use crate::model::{
    DesignDocument, DesignObject, Geometry, ImageFit, ImageProperties, LineProperties, ObjectId, ObjectKind, PathProperties,
    ShapeProperties, TextProperties, TextSizeMode, DEFAULT_BACKGROUND, DEFAULT_CANVAS_HEIGHT, DEFAULT_CANVAS_WIDTH, DUPLICATE_OFFSET,
    MIN_CANVAS_SIZE,
};

/// An imported image is scaled down (never up) so its larger dimension is
/// at most this many canvas units — "a reasonable size that fits within
/// the canvas" for an 800x600 artboard, while an already-small image keeps
/// its natural size.
const IMPORT_MAX_DIMENSION: f64 = 360.0;

pub const MIN_ZOOM: f64 = 0.1;
pub const MAX_ZOOM: f64 = 8.0;
pub const DEFAULT_ZOOM: f64 = 1.0;

/// The display (width, height) for a freshly imported image, preserving
/// its natural aspect ratio.
fn fit_import_size(natural_width: f64, natural_height: f64) -> (f64, f64) {
    if natural_width <= 0.0 || natural_height <= 0.0 {
        return (IMPORT_MAX_DIMENSION, IMPORT_MAX_DIMENSION);
    }
    let scale = (IMPORT_MAX_DIMENSION / natural_width.max(natural_height)).min(1.0);
    (natural_width * scale, natural_height * scale)
}

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

#[derive(Clone, PartialEq)]
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
    /// Whether `document` differs from what's on disk (or, for a never-
    /// saved document, from blank). Derived automatically at the end of
    /// every `reduce` from whether `history`'s undo stack just changed —
    /// see the end of `reduce` for why that's an exact signal — rather
    /// than flagged by hand in each action, so no future action can
    /// forget to set it.
    pub is_dirty: bool,
    /// View-only — never persisted, never affects `is_dirty`, never
    /// touches `document`. 1.0 = 100%. While `fit_mode` is on, this is kept
    /// numerically in sync with the live-computed fit zoom (see
    /// `canvas_area.rs`) purely so every reader (status bar, toolbar) can
    /// keep reading this one field regardless of which mode produced it.
    pub zoom: f64,
    /// Whether the viewport should continuously recompute `zoom` so the
    /// whole document fits the canvas workspace (recalculated on window
    /// resize and on canvas size changes), as opposed to a `zoom` the user
    /// set explicitly. Same "view-only" status as `zoom` itself: never
    /// persisted, never undoable. Any manual zoom action (`SetZoom`) turns
    /// this off; `SetFitMode(true)` (the toolbar's Fit button) turns it
    /// back on.
    pub fit_mode: bool,
    /// Where `document` was last saved to or opened from, if anywhere.
    /// `None` for a never-saved document — Save shows the file dialog in
    /// that case, Save As always does.
    pub file_path: Option<String>,
    /// What the toolbar shows for the document's name — "Untitled" until
    /// the first successful save/open, then the file's name.
    pub document_title: String,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            document: DesignDocument::default(),
            selected_ids: BTreeSet::default(),
            active_tool: Tool::default(),
            history: History::default(),
            clipboard: Vec::default(),
            expanded_groups: BTreeSet::default(),
            editing_text: None,
            is_dirty: false,
            zoom: DEFAULT_ZOOM,
            fit_mode: true,
            file_path: None,
            document_title: "Untitled".to_string(),
        }
    }
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
    /// Imports a freshly decoded file as a brand-new image object,
    /// centered on `center` and sized to fit the canvas — one undo step,
    /// via the same `CreateObject` command every other object uses.
    ImportImage {
        filename: String,
        mime_type: String,
        natural_width: f64,
        natural_height: f64,
        reference: String,
        center: (f64, f64),
    },
    /// One immediate, undoable image-property change from a Properties
    /// panel field (fit mode) — not a new asset.
    CommitImageProperties { id: ObjectId, before: ImageProperties, after: ImageProperties },
    /// Live fill/stroke update during an active color-picker drag (the
    /// saturation/value area, hue slider, or alpha slider) — applied to the
    /// document immediately but NOT recorded to history, the same way
    /// `UpdateGeometry` works for a move/resize drag.
    UpdateShapeProperties { id: ObjectId, properties: ShapeProperties },
    /// One immediate, undoable fill/stroke change — a Properties panel
    /// field edit, or the single step recorded when a color-picker drag
    /// gesture ends.
    CommitShapeProperties { id: ObjectId, before: ShapeProperties, after: ShapeProperties },
    /// Live stroke update during an active color-picker drag on a line —
    /// same "applied but not recorded" pattern as `UpdateShapeProperties`.
    UpdateLineProperties { id: ObjectId, properties: LineProperties },
    /// One immediate, undoable line stroke-color/width change.
    CommitLineProperties { id: ObjectId, before: LineProperties, after: LineProperties },
    /// Live stroke update during an active color-picker drag on a path.
    UpdatePathProperties { id: ObjectId, properties: PathProperties },
    /// One immediate, undoable path stroke-color/width change.
    CommitPathProperties { id: ObjectId, before: PathProperties, after: PathProperties },
    /// "Replace image": registers the freshly decoded file as a new asset
    /// and points `id`'s existing image object at it, leaving its
    /// transform untouched. One undo step.
    ReplaceImageAsset {
        id: ObjectId,
        filename: String,
        mime_type: String,
        natural_width: f64,
        natural_height: f64,
        reference: String,
    },
    Undo,
    Redo,
    /// Resets to a fresh blank document. Never shown directly to a dirty
    /// document without the caller (the New button) having already
    /// resolved the "unsaved changes" prompt.
    NewDocument,
    /// Replaces the document wholesale with one just read from disk.
    /// `path`/`filename` become the new save target/title.
    LoadDocument { document: DesignDocument, path: String, filename: String },
    /// Records where a save just went, without touching the document
    /// itself — used after both Save and Save As succeed.
    MarkSaved { path: String, filename: String },
    /// Absolute zoom level (clamped to `[MIN_ZOOM, MAX_ZOOM]`) — every
    /// *manual* zoom entry point (buttons, presets, wheel, fit-to-selection)
    /// computes its target level and dispatches this rather than each
    /// having its own action. Always turns `fit_mode` off — the user just
    /// asked for a specific zoom, so the viewport should stop overriding it.
    SetZoom(f64),
    /// Turns Fit mode on (the toolbar's Fit button, or the zoom menu's "Fit
    /// to screen") or off. Turning it on doesn't itself compute a zoom
    /// level — `canvas_area.rs` reacts to `fit_mode` being true by
    /// measuring the live workspace and dispatching `SyncFitZoom`.
    SetFitMode(bool),
    /// Internal: keeps `zoom` numerically in sync with the live-computed
    /// fit zoom while `fit_mode` is on. Unlike `SetZoom`, this does NOT
    /// touch `fit_mode` — it's the mechanism fit mode uses to update the
    /// displayed zoom, not a manual override of it.
    SyncFitZoom(f64),
    /// Resizes the document canvas (Canvas Size panel field edit or a
    /// preset), clamped to `MIN_CANVAS_SIZE`. When `scale_content` is set,
    /// every object's geometry is scaled proportionally along with it, as
    /// part of the same undo step; otherwise objects are left exactly
    /// where they are — only the canvas itself changes.
    SetCanvasSize { width: f64, height: f64, scale_content: bool },
    /// Live background-color update during an active color-picker drag —
    /// applied to the document immediately but NOT recorded to history,
    /// the same "live vs. commit" split every other color field uses.
    UpdateBackground(String),
    /// One immediate, undoable background-color change — a hex/RGB field
    /// edit, the eyedropper, or the single step recorded when a
    /// color-picker drag gesture ends.
    CommitBackground { before: String, after: String },
    /// The Canvas Size panel's "Reset to Default" — restores canvas width/
    /// height/background to `DEFAULT_CANVAS_WIDTH`/`DEFAULT_CANVAS_HEIGHT`/
    /// `DEFAULT_BACKGROUND` as one undo step. Never touches objects (same
    /// "resize the canvas, not the content" default as a plain width/
    /// height edit) — only the canvas/background fields it shares with
    /// `SetCanvasSize`/`CommitBackground` are affected.
    ResetCanvasDefaults,
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
        let history_before = self.history.undo_count();
        let mut next = (*self).clone();
        // `Some(v)` for actions that need to force a specific dirty state
        // (New/Load/MarkSaved) rather than let the generic undo-stack
        // check below decide it.
        let mut force_dirty: Option<bool> = None;
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

            EditorAction::ImportImage { filename, mime_type, natural_width, natural_height, reference, center } => {
                let asset_id = next.document.assets.insert(filename.clone(), mime_type, natural_width, natural_height, reference);
                let (width, height) = fit_import_size(natural_width, natural_height);
                let geometry = Geometry::new(center.0 - width / 2.0, center.1 - height / 2.0, width, height);
                let id = next.document.insert(ObjectKind::Image(ImageProperties { asset_id, fit: ImageFit::default() }), geometry);
                if let Some(object) = next.document.get_mut(id) {
                    object.name = filename;
                    next.history.record(Command::CreateObject { object: object.clone() });
                }
                next.selected_ids = BTreeSet::from([id]);
                next.active_tool = Tool::Select;
            }

            EditorAction::CommitImageProperties { id, before, after } => {
                if before != after {
                    if let Some(object) = next.document.get_mut(id) {
                        object.kind = ObjectKind::Image(after);
                    }
                    next.history.record(Command::SetImageProperties { id, before, after });
                }
            }

            EditorAction::UpdateShapeProperties { id, properties } => {
                if let Some(object) = next.document.get_mut(id) {
                    match &mut object.kind {
                        ObjectKind::Rectangle(props) | ObjectKind::Ellipse(props) => *props = properties,
                        _ => {}
                    }
                }
            }

            EditorAction::CommitShapeProperties { id, before, after } => {
                if before != after {
                    if let Some(object) = next.document.get_mut(id) {
                        match &mut object.kind {
                            ObjectKind::Rectangle(props) | ObjectKind::Ellipse(props) => *props = after,
                            _ => {}
                        }
                    }
                    next.history.record(Command::SetShapeProperties { id, before, after });
                }
            }

            EditorAction::UpdateLineProperties { id, properties } => {
                if let Some(object) = next.document.get_mut(id) {
                    if matches!(object.kind, ObjectKind::Line(_)) {
                        object.kind = ObjectKind::Line(properties);
                    }
                }
            }

            EditorAction::CommitLineProperties { id, before, after } => {
                if before != after {
                    if let Some(object) = next.document.get_mut(id) {
                        object.kind = ObjectKind::Line(after);
                    }
                    next.history.record(Command::SetLineProperties { id, before, after });
                }
            }

            EditorAction::UpdatePathProperties { id, properties } => {
                if let Some(object) = next.document.get_mut(id) {
                    if matches!(object.kind, ObjectKind::Path(_)) {
                        object.kind = ObjectKind::Path(properties);
                    }
                }
            }

            EditorAction::CommitPathProperties { id, before, after } => {
                if before != after {
                    if let Some(object) = next.document.get_mut(id) {
                        object.kind = ObjectKind::Path(after.clone());
                    }
                    next.history.record(Command::SetPathProperties { id, before, after });
                }
            }

            EditorAction::ReplaceImageAsset { id, filename, mime_type, natural_width, natural_height, reference } => {
                if let Some(ObjectKind::Image(before)) = next.document.get(id).map(|o| o.kind.clone()) {
                    let asset_id = next.document.assets.insert(filename, mime_type, natural_width, natural_height, reference);
                    let after = ImageProperties { asset_id, ..before };
                    if let Some(object) = next.document.get_mut(id) {
                        object.kind = ObjectKind::Image(after);
                    }
                    next.history.record(Command::SetImageProperties { id, before, after });
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

            EditorAction::NewDocument => {
                next.document = DesignDocument::default();
                next.selected_ids = BTreeSet::new();
                next.active_tool = Tool::Select;
                next.history = History::default();
                next.editing_text = None;
                next.expanded_groups = BTreeSet::new();
                next.file_path = None;
                next.document_title = "Untitled".to_string();
                next.zoom = DEFAULT_ZOOM;
                next.fit_mode = true;
                force_dirty = Some(false);
            }

            EditorAction::LoadDocument { document, path, filename } => {
                // `expanded_groups` is never part of the saved file (it's
                // pure Layers-panel UI state — see `EditorState`'s docs),
                // so it isn't restored from anywhere; it's derived fresh
                // from the loaded document's own group objects instead.
                // Every group (nested ones included — `ObjectKind::Group`
                // doesn't distinguish depth) starts expanded, exactly like
                // `GroupSelected` already expands a group the moment it's
                // created in the current session. Without this, a loaded
                // group starts collapsed while a freshly created one
                // doesn't, which is what actually made a reopened group
                // look impossible to "enter" — its children were still
                // fully selectable/editable the whole time, just hidden
                // behind an unfamiliar collapsed Layers row.
                next.expanded_groups = document.objects.iter().filter(|object| object.kind.is_group()).map(|object| object.id).collect();
                next.document = document;
                next.selected_ids = BTreeSet::new();
                next.active_tool = Tool::Select;
                next.history = History::default();
                next.editing_text = None;
                next.file_path = Some(path);
                next.document_title = filename;
                next.zoom = DEFAULT_ZOOM;
                next.fit_mode = true;
                force_dirty = Some(false);
            }

            EditorAction::MarkSaved { path, filename } => {
                next.file_path = Some(path);
                next.document_title = filename;
                force_dirty = Some(false);
            }

            EditorAction::SetZoom(zoom) => {
                next.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
                next.fit_mode = false;
            }

            EditorAction::SetFitMode(enabled) => {
                next.fit_mode = enabled;
            }

            EditorAction::SyncFitZoom(zoom) => {
                next.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
            }

            EditorAction::SetCanvasSize { width, height, scale_content } => {
                let before = (next.document.canvas_width, next.document.canvas_height);
                let after = (width.max(MIN_CANVAS_SIZE), height.max(MIN_CANVAS_SIZE));
                if before != after {
                    let mut commands = vec![Command::SetCanvasSize { before, after }];
                    if scale_content {
                        let scale_x = after.0 / before.0;
                        let scale_y = after.1 / before.1;
                        let mut geometry_changes = Vec::with_capacity(next.document.objects.len());
                        for object in &next.document.objects {
                            let g = object.geometry;
                            let scaled = Geometry {
                                x: g.x * scale_x,
                                y: g.y * scale_y,
                                width: g.width * scale_x,
                                height: g.height * scale_y,
                                ..g
                            };
                            geometry_changes.push((object.id, g, scaled));
                        }
                        for (id, _, after) in &geometry_changes {
                            if let Some(object) = next.document.get_mut(*id) {
                                object.geometry = *after;
                            }
                        }
                        commands.extend(geometry_changes.into_iter().map(|(id, before, after)| Command::SetGeometry { id, before, after }));
                    }
                    next.document.canvas_width = after.0;
                    next.document.canvas_height = after.1;
                    if let Some(command) = batch(commands) {
                        next.history.record(command);
                    }
                }
            }

            EditorAction::UpdateBackground(color) => {
                next.document.background = color;
            }

            EditorAction::CommitBackground { before, after } => {
                if before != after {
                    next.document.background = after.clone();
                    next.history.record(Command::SetBackground { before, after });
                }
            }

            EditorAction::ResetCanvasDefaults => {
                let mut commands = Vec::new();

                let before_size = (next.document.canvas_width, next.document.canvas_height);
                let after_size = (DEFAULT_CANVAS_WIDTH, DEFAULT_CANVAS_HEIGHT);
                if before_size != after_size {
                    next.document.canvas_width = after_size.0;
                    next.document.canvas_height = after_size.1;
                    commands.push(Command::SetCanvasSize { before: before_size, after: after_size });
                }

                let before_background = next.document.background.clone();
                if before_background != DEFAULT_BACKGROUND {
                    next.document.background = DEFAULT_BACKGROUND.to_string();
                    commands.push(Command::SetBackground { before: before_background, after: DEFAULT_BACKGROUND.to_string() });
                }

                if let Some(command) = batch(commands) {
                    next.history.record(command);
                }
            }
        }

        next.is_dirty = force_dirty.unwrap_or_else(|| next.is_dirty || next.history.undo_count() != history_before);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShapeProperties;

    /// The id, among `ids`, whose object is a group — panics if none is,
    /// since every assertion below needs to tell the group apart from its
    /// duplicated/pasted children without assuming any particular id
    /// ordering (grouping always creates the group *after* its children,
    /// but `expand_with_descendants` re-sorts by id before duplicating/
    /// copying, so the group is not reliably first or last).
    fn group_among(state: &EditorState, ids: &BTreeSet<ObjectId>) -> ObjectId {
        ids.iter().copied().find(|id| state.document.get(*id).is_some_and(|o| o.kind.is_group())).expect("selection should include a group")
    }

    /// Builds a document with two objects grouped together, round-trips it
    /// through JSON exactly like Save/Open does (`document_io.rs` does
    /// nothing more than this for non-image fields), then drives the *real*
    /// reducer through select/move/undo/redo/ungroup/regroup/duplicate/
    /// copy/paste/delete — the full set of group operations a user would
    /// exercise on a freshly reopened file. None of it should behave any
    /// differently than it would in a document that was never saved, since
    /// `LoadDocument` hands the reopened `DesignDocument` to a fresh
    /// `EditorState` the exact same way this test does.
    #[test]
    fn a_group_behaves_identically_after_a_save_open_round_trip() {
        let mut document = DesignDocument::default();
        let a = document.insert(ObjectKind::Rectangle(ShapeProperties::default()), Geometry::new(0.0, 0.0, 50.0, 50.0));
        let b = document.insert(ObjectKind::Ellipse(ShapeProperties::default()), Geometry::new(100.0, 0.0, 50.0, 50.0));
        let (group_object, _) = document.group(&[a, b]).expect("grouping 2 objects should succeed");
        let group_id = group_object.id;

        let saved = serde_json::to_string(&document).expect("document should serialize");
        let reopened: DesignDocument = serde_json::from_str(&saved).expect("document should deserialize");
        assert_eq!(document, reopened, "the document itself must round-trip exactly before any editor behavior is even tested");

        // Mirrors exactly what `EditorAction::LoadDocument` produces: a
        // fresh `EditorState` wrapping the reopened document.
        let mut state = Rc::new(EditorState { document: reopened, ..EditorState::default() });

        // Select the group.
        state = state.reduce(EditorAction::SetSelection(BTreeSet::from([group_id])));
        assert_eq!(state.selected_ids, BTreeSet::from([group_id]));

        // Move the group — mirrors dragging it on canvas: every leaf
        // descendant's geometry shifts by the same delta, one undo step.
        let leaves: Vec<ObjectId> =
            state.document.descendants_of(group_id).into_iter().filter(|id| !state.document.get(*id).unwrap().kind.is_group()).collect();
        assert_eq!(leaves.len(), 2, "the group's two original children should still be its descendants after reopening");
        let changes: Vec<(ObjectId, Geometry, Geometry)> = leaves
            .iter()
            .map(|&id| {
                let before = state.document.get(id).unwrap().geometry;
                (id, before, Geometry { x: before.x + 10.0, y: before.y + 5.0, ..before })
            })
            .collect();
        state = state.reduce(EditorAction::CommitGeometries(changes));
        assert_eq!(state.document.get(a).unwrap().geometry.x, 10.0);
        assert_eq!(state.document.get(b).unwrap().geometry.x, 110.0);
        assert!(state.history.can_undo());

        // Undo the move, then redo it.
        state = state.reduce(EditorAction::Undo);
        assert_eq!(state.document.get(a).unwrap().geometry.x, 0.0);
        assert_eq!(state.document.get(b).unwrap().geometry.x, 100.0);
        state = state.reduce(EditorAction::Redo);
        assert_eq!(state.document.get(a).unwrap().geometry.x, 10.0);

        // Ungroup.
        state = state.reduce(EditorAction::SetSelection(BTreeSet::from([group_id])));
        state = state.reduce(EditorAction::UngroupSelected);
        assert!(state.document.get(group_id).is_none(), "the group object itself should be gone");
        assert_eq!(state.document.get(a).unwrap().parent_id, None);
        assert_eq!(state.document.get(b).unwrap().parent_id, None);
        assert_eq!(state.selected_ids, BTreeSet::from([a, b]));

        // Undo the ungroup: the original group object comes back intact.
        state = state.reduce(EditorAction::Undo);
        assert!(state.document.get(group_id).is_some());
        assert_eq!(state.document.get(a).unwrap().parent_id, Some(group_id));

        // Redo the ungroup, then regroup from scratch (a new group id,
        // since `a`/`b` are top-level again).
        state = state.reduce(EditorAction::Redo);
        state = state.reduce(EditorAction::SetSelection(BTreeSet::from([a, b])));
        state = state.reduce(EditorAction::GroupSelected);
        assert_eq!(state.selected_ids.len(), 1);
        let new_group_id = *state.selected_ids.iter().next().unwrap();
        assert!(state.document.get(new_group_id).unwrap().kind.is_group());
        assert_eq!(state.document.get(a).unwrap().parent_id, Some(new_group_id));

        // Duplicate the group — selection becomes the group *and* its
        // freshly duplicated children (not just the group), same as
        // duplicating any other object always selects everything created.
        state = state.reduce(EditorAction::DuplicateObjects(vec![new_group_id]));
        assert_eq!(state.selected_ids.len(), 3);
        let duplicated_group_id = group_among(&state, &state.selected_ids);
        assert_ne!(duplicated_group_id, new_group_id);
        assert_eq!(state.document.children_of(Some(duplicated_group_id)).count(), 2);

        // Copy the duplicated group (+ its children), then paste it.
        state = state.reduce(EditorAction::Copy);
        assert_eq!(state.clipboard.len(), 3, "copy should include the group and both of its children");
        state = state.reduce(EditorAction::Paste);
        assert_eq!(state.selected_ids.len(), 3);
        let pasted_group_id = group_among(&state, &state.selected_ids);
        assert_eq!(state.document.children_of(Some(pasted_group_id)).count(), 2);

        // Delete the pasted group by id alone — deletion must cascade to
        // its children the same way it does for a group made this session.
        let before_count = state.document.objects.len();
        state = state.reduce(EditorAction::DeleteObjects(vec![pasted_group_id]));
        assert_eq!(state.document.objects.len(), before_count - 3);
        assert!(state.document.get(pasted_group_id).is_none());

        // Undo the delete: the group and both children come back exactly.
        state = state.reduce(EditorAction::Undo);
        assert!(state.document.get(pasted_group_id).is_some());
        assert_eq!(state.document.children_of(Some(pasted_group_id)).count(), 2);
    }

    /// `DesignDocument::group` has no special case that forbids a group
    /// from being one of the ids grouped — nesting is already structurally
    /// supported by the same flat `parent_id`-per-object model regular
    /// grouping uses (see `model.rs`'s module docs). This confirms a
    /// nested group's parent/child chain — inner group, its 2 leaves, and
    /// the outer group wrapping the inner group plus a third leaf —
    /// survives a JSON round-trip with every relationship intact.
    #[test]
    fn nested_groups_survive_a_save_open_round_trip() {
        let mut document = DesignDocument::default();
        let a = document.insert(ObjectKind::Rectangle(ShapeProperties::default()), Geometry::new(0.0, 0.0, 20.0, 20.0));
        let b = document.insert(ObjectKind::Ellipse(ShapeProperties::default()), Geometry::new(30.0, 0.0, 20.0, 20.0));
        let (inner_group, _) = document.group(&[a, b]).expect("inner grouping should succeed");
        let c = document.insert(ObjectKind::Rectangle(ShapeProperties::default()), Geometry::new(0.0, 30.0, 20.0, 20.0));
        let (outer_group, _) = document.group(&[inner_group.id, c]).expect("outer grouping should succeed");

        let saved = serde_json::to_string(&document).expect("document should serialize");
        let reopened: DesignDocument = serde_json::from_str(&saved).expect("document should deserialize");
        assert_eq!(document, reopened);

        assert_eq!(reopened.get(inner_group.id).unwrap().parent_id, Some(outer_group.id));
        assert_eq!(reopened.get(a).unwrap().parent_id, Some(inner_group.id));
        assert_eq!(reopened.get(b).unwrap().parent_id, Some(inner_group.id));
        assert_eq!(reopened.get(c).unwrap().parent_id, Some(outer_group.id));

        // `descendants_of` the outer group must walk through the nested
        // inner group to find every leaf, the same as it would pre-save.
        let mut descendants = reopened.descendants_of(outer_group.id);
        descendants.sort_unstable();
        let mut expected = vec![inner_group.id, a, b, c];
        expected.sort_unstable();
        assert_eq!(descendants, expected);

        // Selecting and moving the outer group should move all 3 leaves
        // (a, b, c) — the inner group is not itself a "leaf" to move.
        let state = Rc::new(EditorState { document: reopened, ..EditorState::default() });
        let leaves: Vec<ObjectId> =
            state.document.descendants_of(outer_group.id).into_iter().filter(|id| !state.document.get(*id).unwrap().kind.is_group()).collect();
        let mut sorted_leaves = leaves.clone();
        sorted_leaves.sort_unstable();
        let mut expected_leaves = vec![a, b, c];
        expected_leaves.sort_unstable();
        assert_eq!(sorted_leaves, expected_leaves);
    }

    /// The actual difference between a group made this session and one
    /// just reopened: `GroupSelected` auto-expands a freshly created group
    /// in the Layers panel (`next.expanded_groups.insert(...)`), but
    /// `LoadDocument` used to always start with `expanded_groups` empty —
    /// so a loaded group rendered collapsed, hiding its children behind an
    /// easy-to-miss caret toggle. Nothing about selection, movement, or
    /// editing was ever actually broken (see the two tests above), but a
    /// user couldn't tell that without knowing to click the caret first.
    /// `expanded_groups` must never be *persisted* (it's UI state, not
    /// part of `DesignDocument`), so this drives it through the real
    /// `LoadDocument` action and checks it's derived fresh from the loaded
    /// document's own groups — including a nested one — every group
    /// starts expanded, exactly like a freshly created one would.
    #[test]
    fn loading_a_document_expands_every_group_including_nested_ones() {
        let mut document = DesignDocument::default();
        let a = document.insert(ObjectKind::Rectangle(ShapeProperties::default()), Geometry::new(0.0, 0.0, 20.0, 20.0));
        let b = document.insert(ObjectKind::Ellipse(ShapeProperties::default()), Geometry::new(30.0, 0.0, 20.0, 20.0));
        let (inner_group, _) = document.group(&[a, b]).expect("inner grouping should succeed");
        let c = document.insert(ObjectKind::Rectangle(ShapeProperties::default()), Geometry::new(0.0, 30.0, 20.0, 20.0));
        let (outer_group, _) = document.group(&[inner_group.id, c]).expect("outer grouping should succeed");

        let saved = serde_json::to_string(&document).expect("document should serialize");
        let reopened: DesignDocument = serde_json::from_str(&saved).expect("document should deserialize");

        let state = Rc::new(EditorState::default());
        let state = state.reduce(EditorAction::LoadDocument { document: reopened, path: "test.apollo".to_string(), filename: "test".to_string() });

        assert_eq!(
            state.expanded_groups,
            BTreeSet::from([inner_group.id, outer_group.id]),
            "every group in a freshly loaded document should start expanded, inner and outer alike"
        );

        // With both levels expanded, the Layers panel would render Rectangle
        // A/B and the Text/Image equivalents as clickable rows the same way
        // it does in a session where the groups were never saved — proven
        // here by confirming a nested leaf is selectable and editable
        // exactly like any other single object, via the same `SetSelection`
        // + `CommitGeometries` path the Properties panel and canvas both use.
        let state = state.reduce(EditorAction::SetSelection(BTreeSet::from([a])));
        assert_eq!(state.selected_ids, BTreeSet::from([a]));
        let before = state.document.get(a).unwrap().geometry;
        let after = Geometry { x: before.x + 25.0, ..before };
        let state = state.reduce(EditorAction::CommitGeometries(vec![(a, before, after)]));
        assert_eq!(state.document.get(a).unwrap().geometry.x, 25.0);
        assert_eq!(state.document.get(b).unwrap().geometry.x, 30.0, "moving one nested child must not move its sibling");
    }
}
