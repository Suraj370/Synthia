//! Browser-native local image import: turns a `web_sys::File` (from a
//! drag-and-drop or a file `<input>`, never from any network request) into
//! a decoded asset plus an `EditorAction` dispatch. Kept as the one place
//! that talks to raw JS interop (object URLs, an offscreen `<img>` load)
//! so `canvas_area.rs`, `top_toolbar.rs`, and `right_panel.rs` each just
//! call `import_image_file` and never touch `wasm-bindgen`/`web-sys`
//! loading mechanics directly.
//!
//! No upload, no external image service: `URL.createObjectURL` hands back
//! a handle to the browser's own in-memory decode of the file the user
//! picked from their own disk — see `asset_manager.rs` for why that's the
//! efficient choice over reading the file into a base64 string.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{DataTransfer, DragEvent, File, FileList, HtmlImageElement, HtmlInputElement, Url};

use crate::editor_state::{EditorAction, EditorContext};
use crate::model::ObjectId;

/// The three raster formats Apollo imports. Anything else (SVG, PDF,
/// random files dragged in by mistake) is silently ignored rather than
/// erroring — a design tool's drop zone shouldn't need a toast for "that
/// wasn't a picture."
pub fn is_supported_image_type(mime_type: &str) -> bool {
    matches!(mime_type, "image/png" | "image/jpeg" | "image/webp")
}

/// What a successfully decoded file should become.
pub enum ImportTarget {
    /// Create a brand-new image object centered on this canvas-space
    /// point.
    NewObject { center: (f64, f64) },
    /// Swap the asset behind an existing image object, keeping its
    /// transform untouched.
    Replace { object_id: ObjectId },
}

/// Reads `file`'s dimensions via an offscreen `<img>` load, then dispatches
/// the appropriate `EditorAction` once decoding finishes. Fire-and-forget:
/// callers don't get a completion signal because the *document* is the
/// completion signal — same as every other editor action.
pub fn import_image_file(editor: EditorContext, file: File, target: ImportTarget) {
    let mime_type = file.type_();
    if !is_supported_image_type(&mime_type) {
        return;
    }
    let Ok(reference) = Url::create_object_url_with_blob(&file) else {
        return;
    };
    let filename = file.name();

    let Ok(probe) = HtmlImageElement::new() else {
        return;
    };
    let onload_probe = probe.clone();
    let onload_reference = reference.clone();
    let onload: Closure<dyn FnMut()> = Closure::once(move || {
        let natural_width = onload_probe.natural_width() as f64;
        let natural_height = onload_probe.natural_height() as f64;
        match &target {
            ImportTarget::NewObject { center } => {
                editor.dispatch(EditorAction::ImportImage {
                    filename,
                    mime_type,
                    natural_width,
                    natural_height,
                    reference: onload_reference,
                    center: *center,
                });
            }
            ImportTarget::Replace { object_id } => {
                editor.dispatch(EditorAction::ReplaceImageAsset {
                    id: *object_id,
                    filename,
                    mime_type,
                    natural_width,
                    natural_height,
                    reference: onload_reference,
                });
            }
        }
    });
    probe.set_onload(Some(onload.as_ref().unchecked_ref()));
    // The closure only needs to live long enough to fire once; `forget`
    // hands its memory to the JS side instead of dropping it when this
    // function returns (there's nothing left here to hold onto it).
    onload.forget();
    probe.set_src(&reference);
}

/// Every accepted-type file out of a `<input type="file">` change event.
pub fn files_from_input(input: &HtmlInputElement) -> Vec<File> {
    files_from_list(input.files())
}

/// Every accepted-type file out of a drop event's `DataTransfer`.
pub fn files_from_drop(event: &DragEvent) -> Vec<File> {
    files_from_list(event.data_transfer().and_then(|dt: DataTransfer| dt.files()))
}

fn files_from_list(list: Option<FileList>) -> Vec<File> {
    let Some(list) = list else {
        return Vec::new();
    };
    (0..list.length()).filter_map(|i| list.get(i)).filter(|file| is_supported_image_type(&file.type_())).collect()
}
