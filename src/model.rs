//! Document model for the design canvas.
//!
//! Deliberately framework-agnostic: nothing here knows about SVG, Yew, or
//! rendering. The renderer and editor state read/write this model, which
//! keeps the door open for undo/redo, serialization, or a different
//! renderer later without touching this file.

pub type ObjectId = u64;

/// The smallest width/height an object may have. Enforced everywhere a
/// geometry is written (interactive resize, Properties panel edits) so the
/// invariant lives in one place.
pub const MIN_OBJECT_SIZE: f64 = 8.0;

/// How far a duplicated object is offset from its source, so the copy is
/// visibly distinct instead of sitting exactly on top of the original.
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
    Text { content: String },
    ImagePlaceholder,
}

impl ObjectKind {
    pub fn type_label(&self) -> &'static str {
        match self {
            ObjectKind::Rectangle => "Rectangle",
            ObjectKind::Ellipse => "Ellipse",
            ObjectKind::Text { .. } => "Text",
            ObjectKind::ImagePlaceholder => "Image",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DesignObject {
    pub id: ObjectId,
    pub name: String,
    pub kind: ObjectKind,
    pub geometry: Geometry,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct DesignDocument {
    pub objects: Vec<DesignObject>,
    next_id: ObjectId,
}

impl DesignDocument {
    /// Adds a new object to the document and returns its id.
    pub fn insert(&mut self, kind: ObjectKind, geometry: Geometry) -> ObjectId {
        self.next_id += 1;
        let id = self.next_id;
        let name = format!("{} {}", kind.type_label(), id);
        self.objects.push(DesignObject {
            id,
            name,
            kind,
            geometry,
        });
        id
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

    /// Inserts a copy of `id`, offset from the source, and returns the new
    /// object's id. Reuses `insert` so the copy gets its own auto-generated
    /// name and id, exactly like a freshly created object.
    pub fn duplicate(&mut self, id: ObjectId) -> Option<ObjectId> {
        let source = self.get(id)?;
        let kind = source.kind.clone();
        let mut geometry = source.geometry;
        geometry.x += DUPLICATE_OFFSET;
        geometry.y += DUPLICATE_OFFSET;
        Some(self.insert(kind, geometry))
    }
}
