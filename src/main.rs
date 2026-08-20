mod app;
mod components;
mod editor_state;
mod model;

use app::App;

fn main() {
    console_error_panic_hook::set_once();
    yew::Renderer::<App>::new().render();
}
