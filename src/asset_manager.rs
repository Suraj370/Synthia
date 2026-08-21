//! Local image asset registry.
//!
//! An `Image` object in the document never carries pixel data itself —
//! only an `AssetId`. The actual bytes live here, one entry per imported
//! file, so multiple objects (or a duplicated/grouped copy) can reference
//! the same picture without copying it, and so undo/redo on an object's
//! transform never has image data anywhere near it.
//!
//! `reference` is deliberately just a string the renderer hands straight
//! to an SVG `<image href>`: today that's always a browser object URL
//! (`URL.createObjectURL`, see `image_import.rs`), which points at the
//! browser's own decoded copy of the file — importing a 10MB photo costs
//! one decode and one short-lived handle, not a base64 string duplicated
//! into every object, into history, and into memory again per render.
//! A future on-disk-backed build (Tauri's `fs`/asset-protocol) would only
//! need to change what `image_import.rs` puts in this field — everything
//! downstream (the document, the renderer, history) is already agnostic
//! to where the string points.
//!
//! That tradeoff does mean `reference` doesn't survive a save/reopen: a
//! browser object URL is only valid for the session that created it. Save
//! (`document_io.rs`) still writes every asset's metadata faithfully (so
//! the format round-trips completely and the object/geometry/filename are
//! all intact), but a reopened file's images will render blank until
//! re-imported, until a real on-disk asset store lands — a known,
//! disclosed gap, not a silent one.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub type AssetId = u64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub id: AssetId,
    pub filename: String,
    pub mime_type: String,
    /// Natural pixel dimensions of the source file, independent of
    /// whatever size an object displaying it is currently set to.
    pub width: f64,
    pub height: f64,
    /// Where the renderer can load pixels from — see module docs.
    pub reference: String,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct AssetManager {
    assets: HashMap<AssetId, Asset>,
    next_id: AssetId,
}

impl AssetManager {
    /// Registers a newly imported file and returns its id. Never removed
    /// by undo (an orphaned entry after undoing an import/replace is inert
    /// — nothing references it — the same simplification `DesignDocument`
    /// already makes for its own id counter).
    pub fn insert(&mut self, filename: String, mime_type: String, width: f64, height: f64, reference: String) -> AssetId {
        self.next_id += 1;
        let id = self.next_id;
        self.assets.insert(id, Asset { id, filename, mime_type, width, height, reference });
        id
    }

    pub fn get(&self, id: AssetId) -> Option<&Asset> {
        self.assets.get(&id)
    }
}
