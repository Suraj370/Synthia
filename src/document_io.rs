//! Document save/open, on top of `tauri_bridge`'s generic invoke.
//!
//! `DesignDocument` is the source of truth this reads and writes — never
//! SVG markup, never a subset. Serialization is plain `serde_json` on a
//! type that already derives `Serialize`/`Deserialize`, so there's no
//! separate "file format" struct to keep in sync with the model; the
//! model *is* the format, versioned via `DesignDocument::version`.
//!
//! One field needs help at this boundary, though: an `Asset`'s `reference`
//! (see `asset_manager.rs`) is a browser object URL while a document is
//! being edited — cheap to clone on every reducer dispatch, but only valid
//! for the session that created it, so it can't be serialized as-is. `save`
//! resolves every asset's bytes into a self-contained `data:` URL, but only
//! in the copy of the document it actually serializes — the live, in-memory
//! document keeps its cheap object URLs untouched. `open` does the reverse
//! immediately after parsing: every `data:` URL becomes a fresh object URL
//! before the document ever becomes the live one, so editing a reopened
//! document is exactly as cheap as editing one that was never saved.

use serde::{Deserialize, Serialize};

use crate::asset_manager;
use crate::model::DesignDocument;
use crate::tauri_bridge::{invoke, InvokeError};

#[derive(Serialize)]
struct SaveArgs {
    contents: String,
    existing_path: Option<String>,
    suggested_name: Option<String>,
}

#[derive(Deserialize)]
struct SaveResponse {
    path: String,
    filename: String,
}

#[derive(Deserialize)]
struct OpenResponse {
    path: String,
    filename: String,
    contents: String,
}

#[derive(Serialize)]
struct NoArgs {}

pub enum SaveOutcome {
    Saved { path: String, filename: String },
    /// The user closed the save dialog without picking a location — not
    /// an error, nothing should change.
    Cancelled,
}

pub enum OpenOutcome {
    Opened { document: DesignDocument, path: String, filename: String },
    /// The user closed the open dialog without picking a file.
    Cancelled,
}

/// Writes `document` to `existing_path` if given, otherwise shows the
/// native save dialog first. Passing `existing_path: None` is also how
/// "Save As" always re-prompts even for an already-saved document.
pub async fn save(document: &DesignDocument, existing_path: Option<String>, suggested_name: Option<String>) -> Result<SaveOutcome, String> {
    // A one-off clone, resolved to self-contained `data:` URLs purely for
    // this write — the caller's live `document` (and its cheap object-URL
    // asset references) is never touched, so this doesn't change what the
    // next reducer dispatch clones.
    let mut to_save = document.clone();
    for id in to_save.assets.ids().collect::<Vec<_>>() {
        if let Some(asset) = to_save.assets.get(id) {
            let data_url = asset_manager::resolve_to_data_url(&asset.reference, &asset.mime_type).await;
            to_save.assets.set_reference(id, data_url);
        }
    }

    let contents = serde_json::to_string_pretty(&to_save).map_err(|error| format!("Could not prepare the document for saving: {error}"))?;
    let args = SaveArgs { contents, existing_path, suggested_name };

    match invoke::<_, Option<SaveResponse>>("save_document", &args).await {
        Ok(Some(response)) => Ok(SaveOutcome::Saved { path: response.path, filename: response.filename }),
        Ok(None) => Ok(SaveOutcome::Cancelled),
        Err(InvokeError::Unavailable) => Err(InvokeError::Unavailable.to_string()),
        Err(error) => Err(error.to_string()),
    }
}

/// Shows the native open dialog, reads the chosen file, and parses it as
/// a `DesignDocument`. A malformed or non-Synthia file produces a readable
/// `Err` instead of panicking. `DesignDocument`'s shape/format is
/// unaffected by the Apollo -> Synthia rename, so files saved by earlier
/// Apollo builds parse here exactly the same as ones saved by Synthia.
pub async fn open() -> Result<OpenOutcome, String> {
    match invoke::<_, Option<OpenResponse>>("open_document", &NoArgs {}).await {
        Ok(Some(response)) => {
            let mut document: DesignDocument =
                serde_json::from_str(&response.contents).map_err(|error| format!("This file isn't a valid Synthia document: {error}"))?;

            // Turn each asset's persisted `data:` URL back into a fresh,
            // cheap object URL before this document ever becomes the live
            // one. A reference that *isn't* a `data:` URL (a `blob:` URL
            // from a file saved before this conversion existed, already
            // dead) is left exactly as it is — nothing here to recover it
            // from, and `document_to_svg`/`image_svg` already render
            // nothing for an asset with no resolvable data rather than a
            // broken reference.
            for id in document.assets.ids().collect::<Vec<_>>() {
                if let Some(asset) = document.assets.get(id) {
                    if let Some(object_url) = asset_manager::data_url_to_blob_reference(&asset.reference) {
                        document.assets.set_reference(id, object_url);
                    }
                }
            }

            Ok(OpenOutcome::Opened { document, path: response.path, filename: response.filename })
        }
        Ok(None) => Ok(OpenOutcome::Cancelled),
        Err(error) => Err(error.to_string()),
    }
}
