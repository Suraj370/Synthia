//! Local image asset registry.
//!
//! An `Image` object in the document never carries pixel data itself —
//! only an `AssetId`. The actual bytes live here, one entry per imported
//! file, so multiple objects (or a duplicated/grouped copy) can reference
//! the same picture without copying it, and so undo/redo on an object's
//! transform never has image data anywhere near it.
//!
//! `reference` is deliberately just a string the renderer hands straight
//! to an SVG `<image href>`: while a document is actively being edited,
//! that's always a browser object URL (`URL.createObjectURL`, see
//! `image_import.rs`), which points at the browser's own decoded copy of
//! the file — importing a 10MB photo costs one decode and one short-lived
//! handle, not a base64 string duplicated into every object, into history,
//! and into memory again on every reducer dispatch (`EditorState::reduce`
//! clones the whole `DesignDocument`, `AssetManager` included, on every
//! action — zoom, a drag tick, a keystroke — so keeping `reference` cheap
//! while editing matters).
//!
//! An object URL is only valid for the session that created it, though, so
//! it can't be what actually gets saved to disk. `document_io.rs` handles
//! that boundary instead of this module or the live document ever paying
//! the base64 cost: `save` resolves every asset's bytes into a
//! self-contained `data:` URL (via `resolve_to_data_url`, below) only in
//! the copy of the document it serializes, and `open` immediately converts
//! a loaded document's `data:` URLs back into fresh, cheap object URLs
//! (via `data_url_to_blob_reference`) before the document ever becomes the
//! live one. The two conversions below are what make that possible; they
//! don't know about save/open/export themselves, just how to turn one
//! reference into the other.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use web_sys::Response;

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

    /// Every registered asset's id, independent of whether any object
    /// currently references it — used by `document_io.rs` to resolve/
    /// restore *all* stored assets on save/open, not just the ones a
    /// visible object happens to point at right now.
    pub fn ids(&self) -> impl Iterator<Item = AssetId> + '_ {
        self.assets.keys().copied()
    }

    /// Overwrites `id`'s `reference` in place — used by `document_io.rs`
    /// to swap between the cheap live object-URL form and the
    /// self-contained `data:` URL form at the save/open boundary. A no-op
    /// if `id` isn't registered.
    pub fn set_reference(&mut self, id: AssetId, reference: String) {
        if let Some(asset) = self.assets.get_mut(&id) {
            asset.reference = reference;
        }
    }
}

/// Fetches `reference`'s bytes and re-encodes them as a self-contained
/// `data:` URL — already a no-op passthrough if it's a `data:` URL to
/// begin with. `blob:`/`http(s):` references go through `fetch`, which
/// resolves blob: URLs from the browser's own in-memory registry — no
/// network involved, just the standard API for reading a Blob's bytes back
/// out. Falls back to the original (unresolved) reference on any failure
/// so a broken/expired asset never blocks the rest of a save or export.
pub async fn resolve_to_data_url(reference: &str, mime_type: &str) -> String {
    let fallback = || reference.to_string();
    if reference.starts_with("data:") {
        return fallback();
    }
    let Some(window) = web_sys::window() else { return fallback() };

    let Ok(response_value) = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(reference)).await else {
        return fallback();
    };
    let Ok(response) = response_value.dyn_into::<Response>() else { return fallback() };
    let Ok(buffer_promise) = response.array_buffer() else { return fallback() };
    let Ok(buffer_value) = wasm_bindgen_futures::JsFuture::from(buffer_promise).await else { return fallback() };

    // `btoa` expects a "binary string" — one JS UTF-16 code unit per byte
    // (0-255) — not UTF-8 text, so this isn't a text encoding at all, just
    // the standard byte<->char-code round trip used on both ends of this
    // module (see `data_url_to_blob_reference` for the reverse direction).
    let bytes = js_sys::Uint8Array::new(&buffer_value).to_vec();
    let binary_string: String = bytes.iter().map(|&b| b as char).collect();
    match window.btoa(&binary_string) {
        Ok(base64) => format!("data:{mime_type};base64,{base64}"),
        Err(_) => fallback(),
    }
}

/// Splits a `data:<mime>;base64,<payload>` URL into its mime type and
/// base64 payload. Pure and native-testable on purpose — the actual
/// `Blob`/object-URL creation in `data_url_to_blob_reference` needs a
/// browser and can't be exercised in a native `cargo test` run, but the
/// string parsing that decides *whether* a reference is even convertible
/// can be.
fn parse_data_url(data_url: &str) -> Option<(&str, &str)> {
    let rest = data_url.strip_prefix("data:")?;
    let (header, payload) = rest.split_once(',')?;
    // `header` looks like `image/png;base64` — the `;base64` marker is the
    // only encoding this module ever writes (see `resolve_to_data_url`),
    // so the part before the first `;` is always the mime type.
    let mime_type = header.split(';').next().filter(|s| !s.is_empty())?;
    Some((mime_type, payload))
}

/// The inverse of `resolve_to_data_url` for `data:` references specifically:
/// decodes the base64 payload and creates a fresh, session-local object URL
/// from it — what turns a reopened document's persisted image data back
/// into the cheap reference the live document/editor uses everywhere else
/// (see module docs). Returns `None` for anything that isn't a `data:` URL
/// — including a `blob:` URL from a file saved before this fix existed,
/// which is already dead and has no bytes here to recover; `document_io.rs`
/// leaves such a reference as-is rather than treating it as an error.
pub fn data_url_to_blob_reference(data_url: &str) -> Option<String> {
    let (mime_type, payload) = parse_data_url(data_url)?;
    let window = web_sys::window()?;
    let binary_string = window.atob(payload).ok()?;
    let bytes: Vec<u8> = binary_string.chars().map(|c| c as u8).collect();

    let array = js_sys::Uint8Array::from(bytes.as_slice());
    let parts = js_sys::Array::new();
    parts.push(&array);
    let blob_options = web_sys::BlobPropertyBag::new();
    blob_options.set_type(mime_type);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &blob_options).ok()?;
    web_sys::Url::create_object_url_with_blob(&blob).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_well_formed_data_url() {
        assert_eq!(parse_data_url("data:image/png;base64,AAAA"), Some(("image/png", "AAAA")));
    }

    #[test]
    fn rejects_a_blob_url() {
        assert_eq!(parse_data_url("blob:http://localhost:1420/abc-123"), None);
    }

    #[test]
    fn rejects_a_data_url_with_no_comma() {
        assert_eq!(parse_data_url("data:image/png;base64"), None);
    }
}
