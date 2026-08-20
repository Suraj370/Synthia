//! Document model for the design canvas.
//!
//! Deliberately framework-agnostic: nothing here knows about SVG, Yew, or
//! rendering. The renderer and editor state read/write this model, which
//! keeps the door open for undo/redo, serialization, or a different
//! renderer later without touching this file.
//!
//! Grouping is layered onto the existing flat `objects` list rather than a
//! nested tree: every object (including a `Group`) carries a `parent_id`.
//! Paint/z-order is still just list order, so grouping never has to touch
//! the storage structure that the renderer and undo history already rely
//! on — it only changes which objects report which parent.

use std::collections::HashMap;

use crate::asset_manager::{AssetId, AssetManager};

pub type ObjectId = u64;

/// Bumped whenever the document's shape changes in a way a future
/// load/save path would need to migrate around. Nothing reads or writes a
/// document to disk yet, so this has no behavior today — it exists so
/// that whenever persistence lands, "what format is this" is already a
/// question the model can answer.
pub const DOCUMENT_FORMAT_VERSION: u32 = 1;

/// The smallest width/height an object may have. Enforced everywhere a
/// geometry is written (interactive resize, Properties panel edits) so the
/// invariant lives in one place.
pub const MIN_OBJECT_SIZE: f64 = 8.0;

/// How far a duplicated or pasted object is offset from its source, so the
/// copy is visibly distinct instead of sitting exactly on top of the
/// original.
pub const DUPLICATE_OFFSET: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub rotation: f64,
    pub opacity: f64,
}

impl Geometry {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
            rotation: 0.0,
            opacity: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectKind {
    Rectangle,
    Ellipse,
    Text(TextProperties),
    Image(ImageProperties),
    /// A pure container: has no shape of its own. Its children (objects
    /// whose `parent_id` points at it) render normally in their own list
    /// position; the group only exists so selection/move can treat them as
    /// one unit and the Layers panel can nest them.
    Group,
}

impl ObjectKind {
    pub fn type_label(&self) -> &'static str {
        match self {
            ObjectKind::Rectangle => "Rectangle",
            ObjectKind::Ellipse => "Ellipse",
            ObjectKind::Text(_) => "Text",
            ObjectKind::Image(_) => "Image",
            ObjectKind::Group => "Group",
        }
    }

    pub fn is_group(&self) -> bool {
        matches!(self, ObjectKind::Group)
    }
}

/// The small set of system/web-safe font families this build offers.
/// Deliberately not an open-ended font picker: no fonts are ever
/// downloaded, so every option here has to already exist on the user's
/// system. Each variant's `css_stack` lists its own graceful fallbacks, so
/// if the named face isn't installed the browser silently substitutes a
/// close system font instead of failing to render.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontFamily {
    #[default]
    Inter,
    Arial,
    Helvetica,
    Georgia,
    TimesNewRoman,
    CourierNew,
    SystemSansSerif,
    SystemSerif,
}

impl FontFamily {
    pub const ALL: [FontFamily; 8] = [
        FontFamily::Inter,
        FontFamily::Arial,
        FontFamily::Helvetica,
        FontFamily::Georgia,
        FontFamily::TimesNewRoman,
        FontFamily::CourierNew,
        FontFamily::SystemSansSerif,
        FontFamily::SystemSerif,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FontFamily::Inter => "Inter",
            FontFamily::Arial => "Arial",
            FontFamily::Helvetica => "Helvetica",
            FontFamily::Georgia => "Georgia",
            FontFamily::TimesNewRoman => "Times New Roman",
            FontFamily::CourierNew => "Courier New",
            FontFamily::SystemSansSerif => "System Sans-Serif",
            FontFamily::SystemSerif => "System Serif",
        }
    }

    /// CSS `font-family` value, always ending in a generic family so an
    /// unavailable face falls back to a real system font.
    pub fn css_stack(self) -> &'static str {
        match self {
            FontFamily::Inter => "'Inter', -apple-system, 'Segoe UI', sans-serif",
            FontFamily::Arial => "Arial, Helvetica, sans-serif",
            FontFamily::Helvetica => "Helvetica, Arial, sans-serif",
            FontFamily::Georgia => "Georgia, 'Times New Roman', serif",
            FontFamily::TimesNewRoman => "'Times New Roman', Times, serif",
            FontFamily::CourierNew => "'Courier New', Courier, monospace",
            FontFamily::SystemSansSerif => "-apple-system, 'Segoe UI', Arial, sans-serif",
            FontFamily::SystemSerif => "Georgia, 'Times New Roman', serif",
        }
    }

    pub fn from_label(label: &str) -> Option<FontFamily> {
        Self::ALL.into_iter().find(|f| f.label() == label)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontWeight {
    #[default]
    Regular,
    Medium,
    SemiBold,
    Bold,
}

impl FontWeight {
    pub const ALL: [FontWeight; 4] = [FontWeight::Regular, FontWeight::Medium, FontWeight::SemiBold, FontWeight::Bold];

    pub fn label(self) -> &'static str {
        match self {
            FontWeight::Regular => "Regular",
            FontWeight::Medium => "Medium",
            FontWeight::SemiBold => "SemiBold",
            FontWeight::Bold => "Bold",
        }
    }

    pub fn css_value(self) -> &'static str {
        match self {
            FontWeight::Regular => "400",
            FontWeight::Medium => "500",
            FontWeight::SemiBold => "600",
            FontWeight::Bold => "700",
        }
    }

    pub fn from_label(label: &str) -> Option<FontWeight> {
        Self::ALL.into_iter().find(|w| w.label() == label)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontStyle {
    #[default]
    Regular,
    Italic,
}

impl FontStyle {
    pub fn css_value(self) -> &'static str {
        match self {
            FontStyle::Regular => "normal",
            FontStyle::Italic => "italic",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextDecoration {
    #[default]
    None,
    Underline,
    Strikethrough,
}

impl TextDecoration {
    pub fn css_value(self) -> &'static str {
        match self {
            TextDecoration::None => "none",
            TextDecoration::Underline => "underline",
            TextDecoration::Strikethrough => "line-through",
        }
    }
}

/// Whether a text object's box tracks its content (`Auto`) or the content
/// wraps inside a box the user has explicitly sized (`Fixed`). Dragging a
/// resize handle on an `Auto` text object is what switches it to `Fixed` —
/// there's no separate mode toggle, matching how resizing already implies
/// "I want to control this box's size" for every other object kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextSizeMode {
    #[default]
    Auto,
    Fixed,
}

/// Everything about a text object that isn't its transform (position,
/// size, rotation, opacity — all still plain `Geometry`, shared with every
/// other object kind). Kept as one explicit, typed struct rather than a
/// generic property bag so every text field is checked at compile time.
#[derive(Clone, Debug, PartialEq)]
pub struct TextProperties {
    pub content: String,
    pub font_family: FontFamily,
    pub font_size: f64,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,
    pub text_align: TextAlign,
    /// Multiplier of `font_size`, e.g. 1.2.
    pub line_height: f64,
    /// Extra space between characters, in the same units as `font_size`.
    pub letter_spacing: f64,
    /// CSS color for the glyphs.
    pub fill: String,
    pub text_decoration: TextDecoration,
    pub size_mode: TextSizeMode,
}

impl Default for TextProperties {
    fn default() -> Self {
        Self {
            content: String::new(),
            font_family: FontFamily::default(),
            font_size: 16.0,
            font_weight: FontWeight::default(),
            font_style: FontStyle::default(),
            text_align: TextAlign::default(),
            line_height: 1.2,
            letter_spacing: 0.0,
            fill: "#ececee".to_string(),
            text_decoration: TextDecoration::default(),
            size_mode: TextSizeMode::default(),
        }
    }
}

/// How an image's pixels map onto its (possibly different-aspect-ratio)
/// on-canvas box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ImageFit {
    /// Stretches to exactly fill the box, ignoring aspect ratio.
    #[default]
    Fill,
    /// Scales to fit entirely inside the box, preserving aspect ratio
    /// (may letterbox).
    Fit,
    /// Scales to cover the box, preserving aspect ratio, cropping
    /// whatever overflows.
    Crop,
}

/// Everything about an image object that isn't its transform — mirrors
/// `TextProperties`: the object's `Geometry` still owns position/size/
/// rotation/opacity, this only owns what's specific to being an image.
/// Deliberately just a reference (`asset_id`) rather than pixel data —
/// see `asset_manager.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageProperties {
    pub asset_id: AssetId,
    pub fit: ImageFit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DesignObject {
    pub id: ObjectId,
    pub name: String,
    pub kind: ObjectKind,
    pub geometry: Geometry,
    /// `None` means top-level. `Some(group_id)` means this object is a
    /// direct child of that group.
    pub parent_id: Option<ObjectId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DesignDocument {
    pub objects: Vec<DesignObject>,
    /// Local image data referenced by any `ObjectKind::Image` in
    /// `objects` via its `asset_id` — see `asset_manager.rs`.
    pub assets: AssetManager,
    pub version: u32,
    next_id: ObjectId,
}

impl Default for DesignDocument {
    fn default() -> Self {
        Self {
            objects: Vec::new(),
            assets: AssetManager::default(),
            version: DOCUMENT_FORMAT_VERSION,
            next_id: 0,
        }
    }
}

impl DesignDocument {
    /// Adds a new top-level object to the document and returns its id.
    pub fn insert(&mut self, kind: ObjectKind, geometry: Geometry) -> ObjectId {
        self.next_id += 1;
        let id = self.next_id;
        let name = format!("{} {}", kind.type_label(), id);
        self.insert_object(DesignObject {
            id,
            name,
            kind,
            geometry,
            parent_id: None,
        });
        id
    }

    /// Appends a fully-formed object as-is, preserving its id exactly (used
    /// by undo/redo, where the restored object must keep its original
    /// identity). Advances the id counter past it so future auto-generated
    /// ids never collide.
    pub fn insert_object(&mut self, object: DesignObject) {
        self.next_id = self.next_id.max(object.id);
        self.objects.push(object);
    }

    /// Like `insert_object`, but restores the object at a specific index
    /// instead of the end — used to undo a delete back into its original
    /// z-order position.
    pub fn insert_at(&mut self, index: usize, object: DesignObject) {
        self.next_id = self.next_id.max(object.id);
        let index = index.min(self.objects.len());
        self.objects.insert(index, object);
    }

    pub fn get(&self, id: ObjectId) -> Option<&DesignObject> {
        self.objects.iter().find(|object| object.id == id)
    }

    pub fn get_mut(&mut self, id: ObjectId) -> Option<&mut DesignObject> {
        self.objects.iter_mut().find(|object| object.id == id)
    }

    pub fn remove(&mut self, id: ObjectId) {
        self.objects.retain(|object| object.id != id);
    }

    /// Removes and returns the object along with the index it occupied,
    /// so a caller building an undo command can restore it in place later.
    pub fn remove_with_index(&mut self, id: ObjectId) -> Option<(usize, DesignObject)> {
        let index = self.objects.iter().position(|object| object.id == id)?;
        Some((index, self.objects.remove(index)))
    }

    /// Direct children of `parent` (or the top-level objects, if `parent`
    /// is `None`), in their existing document order.
    pub fn children_of(&self, parent: Option<ObjectId>) -> impl DoubleEndedIterator<Item = &DesignObject> {
        self.objects.iter().filter(move |object| object.parent_id == parent)
    }

    /// All descendants of `id` (children, grandchildren, ...), in no
    /// particular order. Used so deleting/duplicating/moving a group also
    /// covers everything nested inside it.
    pub fn descendants_of(&self, id: ObjectId) -> Vec<ObjectId> {
        let mut result = Vec::new();
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            for child in self.children_of(Some(current)) {
                result.push(child.id);
                stack.push(child.id);
            }
        }
        result
    }

    /// Walks `parent_id` up to the top: clicking any member of a group on
    /// the canvas should act on the whole group, the way mainstream design
    /// tools treat an un-entered group as a single object.
    pub fn outermost_ancestor(&self, id: ObjectId) -> ObjectId {
        let mut current = id;
        while let Some(parent) = self.get(current).and_then(|o| o.parent_id) {
            current = parent;
        }
        current
    }

    /// Axis-aligned bounding box covering every object in `ids`. Rotation
    /// isn't compensated for (the existing single-object resize has the
    /// same simplification), so this is exact only for unrotated content.
    pub fn bounding_box(&self, ids: &[ObjectId]) -> Option<Geometry> {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut found = false;

        for id in ids {
            if let Some(object) = self.get(*id) {
                found = true;
                let g = &object.geometry;
                min_x = min_x.min(g.x);
                min_y = min_y.min(g.y);
                max_x = max_x.max(g.x + g.width);
                max_y = max_y.max(g.y + g.height);
            }
        }

        found.then(|| Geometry::new(min_x, min_y, max_x - min_x, max_y - min_y))
    }

    /// Inserts copies of `sources` as brand-new objects (fresh ids, offset
    /// geometry), remapping `parent_id` references that point at another
    /// object within the same `sources` slice so the copied structure
    /// stays internally consistent — any `parent_id` pointing outside the
    /// slice becomes top-level instead. Shared by `duplicate` and paste,
    /// both of which need to copy a set of objects (possibly a group and
    /// its children) as one unit.
    pub fn insert_many_remapped(&mut self, sources: &[DesignObject], offset_x: f64, offset_y: f64) -> Vec<DesignObject> {
        let mut id_map: HashMap<ObjectId, ObjectId> = HashMap::new();
        let mut new_ids = Vec::with_capacity(sources.len());

        for source in sources {
            let mut geometry = source.geometry;
            geometry.x += offset_x;
            geometry.y += offset_y;
            let new_id = self.insert(source.kind.clone(), geometry);
            id_map.insert(source.id, new_id);
            new_ids.push(new_id);
        }

        for source in sources {
            let new_id = id_map[&source.id];
            let new_parent = source.parent_id.and_then(|old_parent| id_map.get(&old_parent).copied());
            if let Some(object) = self.get_mut(new_id) {
                object.parent_id = new_parent;
            }
        }

        new_ids.into_iter().filter_map(|id| self.get(id).cloned()).collect()
    }

    /// Moves an existing object to `new_index` within the flat document
    /// list (paint/z-order and the Layers panel's ordering are both driven
    /// directly by this list). Returns the index it moved from, so a caller
    /// can build an undo command.
    pub fn move_to_index(&mut self, id: ObjectId, new_index: usize) -> Option<usize> {
        let old_index = self.objects.iter().position(|object| object.id == id)?;
        let object = self.objects.remove(old_index);
        let new_index = new_index.min(self.objects.len());
        self.objects.insert(new_index, object);
        Some(old_index)
    }

    /// Groups `ids` under a new `Group` object sized to their combined
    /// bounding box. Returns the new group plus the `(id, before_parent,
    /// after_parent)` reparenting applied to each grouped object, so the
    /// caller can build an undo command from it. Needs at least 2 ids.
    pub fn group(&mut self, ids: &[ObjectId]) -> Option<(DesignObject, Vec<(ObjectId, Option<ObjectId>, Option<ObjectId>)>)> {
        if ids.len() < 2 {
            return None;
        }
        let bbox = self.bounding_box(ids)?;
        let group_id = self.insert(ObjectKind::Group, bbox);

        let mut changes = Vec::with_capacity(ids.len());
        for &id in ids {
            if let Some(object) = self.get_mut(id) {
                let before = object.parent_id;
                object.parent_id = Some(group_id);
                changes.push((id, before, Some(group_id)));
            }
        }

        let group_object = self.get(group_id)?.clone();
        Some((group_object, changes))
    }
}
