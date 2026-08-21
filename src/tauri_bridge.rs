//! The one place that reaches out of the WASM sandbox to call native
//! Tauri commands (`src-tauri/src/lib.rs`). Everything here is a thin,
//! typed wrapper around `window.__TAURI__.core.invoke` — no filesystem or
//! dialog logic lives here, only the JS interop needed to call into the
//! Rust code that does. `document_io.rs` (document <-> JSON) and the UI
//! (`top_toolbar.rs`) are the only callers.
//!
//! Arguments and results cross the boundary as JSON (`serde_json` <->
//! `JSON.parse`/`stringify`) rather than pulling in `serde-wasm-bindgen`:
//! this path only runs once per Save/Open, not per frame, so the extra
//! string round-trip is free in practice and it's one fewer dependency.
//! Command argument structs use snake_case field names to match; the
//! commands themselves are declared `rename_all = "snake_case"` on the
//! Rust side so the two ends agree without a naming-convention footgun.

use js_sys::JSON;
use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn invoke_raw(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

/// Whether this page is actually running inside the Tauri shell (vs. a
/// plain browser tab, e.g. during `trunk serve` development) — checked
/// before ever calling `invoke_raw`, since that binding simply doesn't
/// exist otherwise and would throw.
pub fn is_tauri_available() -> bool {
    web_sys::window().is_some_and(|window| js_sys::Reflect::has(&window, &JsValue::from_str("__TAURI__")).unwrap_or(false))
}

#[derive(Debug, Clone)]
pub enum InvokeError {
    /// Not running inside Tauri at all — the caller should show a
    /// "requires the desktop app" message rather than a generic failure.
    Unavailable,
    /// The command itself reported an error (a `String` on the Rust side),
    /// or the request/response couldn't be encoded.
    Failed(String),
}

impl std::fmt::Display for InvokeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvokeError::Unavailable => write!(f, "This action requires running Synthia as a desktop app."),
            InvokeError::Failed(message) => write!(f, "{message}"),
        }
    }
}

/// Invokes a Tauri command with a `serde`-serializable argument struct and
/// deserializes its result.
pub async fn invoke<A: Serialize, R: DeserializeOwned>(cmd: &str, args: &A) -> Result<R, InvokeError> {
    if !is_tauri_available() {
        return Err(InvokeError::Unavailable);
    }

    let args_json = serde_json::to_string(args).map_err(|error| InvokeError::Failed(format!("Could not encode request: {error}")))?;
    let args_value = JSON::parse(&args_json).map_err(|_| InvokeError::Failed("Could not encode request".to_string()))?;

    let result = invoke_raw(cmd, args_value).await.map_err(|error| InvokeError::Failed(js_error_to_string(&error)))?;

    let result_json = JSON::stringify(&result).ok().and_then(|s| s.as_string()).unwrap_or_else(|| "null".to_string());
    serde_json::from_str(&result_json).map_err(|error| InvokeError::Failed(format!("Unexpected response: {error}")))
}

fn js_error_to_string(error: &JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}
