//! Native filesystem/dialog layer. Everything here is deliberately dumb:
//! these commands know nothing about Apollo's document shape — they take
//! an opaque JSON string from the frontend and write it, or read a file
//! and hand the string back. The frontend (where `DesignDocument` is
//! defined) owns serialization; this crate only owns "show a native
//! dialog" and "touch the disk", per the project's split between editor/
//! document state (Yew) and filesystem operations (Tauri/Rust).

use tauri_plugin_dialog::DialogExt;

const FILE_FILTER_NAME: &str = "Apollo Design";
const FILE_EXTENSIONS: [&str; 2] = ["apollo", "design"];

#[derive(serde::Serialize)]
struct SaveResult {
    path: String,
    filename: String,
}

#[derive(serde::Serialize)]
struct OpenResult {
    path: String,
    filename: String,
    contents: String,
}

/// Derives a document title (no extension, no directory) from a path the
/// way the frontend displays it in the toolbar.
fn filename_from_path(path: &str) -> String {
    std::path::Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled").to_string()
}

/// Writes `contents` to `existing_path` if given, otherwise shows the
/// native save dialog first and writes to whatever the user picks. Used
/// for both Save (path already known) and Save As (always dialog) — the
/// frontend decides which by whether it passes a path.
///
/// `Ok(None)` means the user cancelled the dialog — not an error.
#[tauri::command(rename_all = "snake_case")]
fn save_document(app: tauri::AppHandle, contents: String, existing_path: Option<String>, suggested_name: Option<String>) -> Result<Option<SaveResult>, String> {
    let path = match existing_path {
        Some(path) => path,
        None => {
            let mut dialog = app.dialog().file().add_filter(FILE_FILTER_NAME, &FILE_EXTENSIONS);
            if let Some(name) = suggested_name {
                dialog = dialog.set_file_name(&format!("{name}.apollo"));
            }
            match dialog.blocking_save_file() {
                Some(file_path) => file_path.to_string(),
                None => return Ok(None),
            }
        }
    };

    std::fs::write(&path, contents).map_err(|error| format!("Could not save file: {error}"))?;

    Ok(Some(SaveResult { filename: filename_from_path(&path), path }))
}

/// Shows the native open dialog and reads the chosen file's contents.
///
/// `Ok(None)` means the user cancelled the dialog — not an error.
#[tauri::command]
fn open_document(app: tauri::AppHandle) -> Result<Option<OpenResult>, String> {
    let file_path = app.dialog().file().add_filter(FILE_FILTER_NAME, &FILE_EXTENSIONS).blocking_pick_file();

    let Some(file_path) = file_path else {
        return Ok(None);
    };
    let path = file_path.to_string();

    let contents = std::fs::read_to_string(&path).map_err(|error| format!("Could not open file: {error}"))?;

    Ok(Some(OpenResult { filename: filename_from_path(&path), path, contents }))
}

#[derive(serde::Serialize)]
struct ExportResult {
    path: String,
}

/// Shows the native save dialog (always — unlike `save_document`, Export
/// has no "existing path" concept, it always asks where to put the new
/// file) and writes `contents` verbatim. Used for both SVG (UTF-8 text,
/// as bytes) and PNG (already-encoded image bytes) — this command doesn't
/// know or care which, exactly like `save_document` doesn't know it's
/// writing a design document.
///
/// `Ok(None)` means the user cancelled the dialog — not an error.
#[tauri::command(rename_all = "snake_case")]
fn export_file(app: tauri::AppHandle, contents: Vec<u8>, suggested_name: String, extension: String, filter_name: String) -> Result<Option<ExportResult>, String> {
    let dialog = app.dialog().file().add_filter(&filter_name, &[extension.as_str()]).set_file_name(&format!("{suggested_name}.{extension}"));

    let Some(file_path) = dialog.blocking_save_file() else {
        return Ok(None);
    };
    let path = file_path.to_string();

    std::fs::write(&path, contents).map_err(|error| format!("Could not export file: {error}"))?;

    Ok(Some(ExportResult { path }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![save_document, open_document, export_file])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
