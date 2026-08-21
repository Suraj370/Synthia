mod app;
mod asset_manager;
mod color;
mod components;
mod document_io;
mod editor_state;
mod export;
mod freehand;
mod history;
mod image_import;
mod model;
mod snapping;
mod tauri_bridge;
mod text_metrics;

use app::App;

fn main() {
    console_error_panic_hook::set_once();
    yew::Renderer::<App>::new().render();
}
