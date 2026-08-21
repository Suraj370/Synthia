//! Document save/open, on top of `tauri_bridge`'s generic invoke.
//!
//! `DesignDocument` is the source of truth this reads and writes — never
//! SVG markup, never a subset. Serialization is plain `serde_json` on a
//! type that already derives `Serialize`/`Deserialize`, so there's no
//! separate "file format" struct to keep in sync with the model; the
//! model *is* the format, versioned via `DesignDocument::version`.

use serde::{Deserialize, Serialize};

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
    let contents = serde_json::to_string_pretty(document).map_err(|error| format!("Could not prepare the document for saving: {error}"))?;
    let args = SaveArgs { contents, existing_path, suggested_name };

    match invoke::<_, Option<SaveResponse>>("save_document", &args).await {
        Ok(Some(response)) => Ok(SaveOutcome::Saved { path: response.path, filename: response.filename }),
        Ok(None) => Ok(SaveOutcome::Cancelled),
        Err(InvokeError::Unavailable) => Err(InvokeError::Unavailable.to_string()),
        Err(error) => Err(error.to_string()),
    }
}

/// Shows the native open dialog, reads the chosen file, and parses it as
/// a `DesignDocument`. A malformed or non-Apollo file produces a readable
/// `Err` instead of panicking.
pub async fn open() -> Result<OpenOutcome, String> {
    match invoke::<_, Option<OpenResponse>>("open_document", &NoArgs {}).await {
        Ok(Some(response)) => {
            let document: DesignDocument =
                serde_json::from_str(&response.contents).map_err(|error| format!("This file isn't a valid Apollo document: {error}"))?;
            Ok(OpenOutcome::Opened { document, path: response.path, filename: response.filename })
        }
        Ok(None) => Ok(OpenOutcome::Cancelled),
        Err(error) => Err(error.to_string()),
    }
}
