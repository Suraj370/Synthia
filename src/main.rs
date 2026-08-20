mod app;
mod asset_manager;
mod components;
mod editor_state;
mod history;
mod image_import;
mod model;
mod snapping;
mod text_metrics;

use app::App;

fn main() {
    console_error_panic_hook::set_once();
    yew::Renderer::<App>::new().render();
}
