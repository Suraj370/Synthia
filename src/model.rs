//! Document model for the design canvas.
//!
//! Deliberately framework-agnostic: nothing here knows about SVG, Yew, or
//! rendering. The renderer and editor state read/write this model, which
//! keeps the door open for undo/redo, serialization, or a different
//! renderer later without touching this file.

pub type ObjectId = u64;

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
}
