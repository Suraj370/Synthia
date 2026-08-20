use crate::components::canvas_area::CanvasArea;
use crate::components::left_panel::LeftPanel;
use crate::components::right_panel::RightPanel;
use crate::components::status_bar::StatusBar;
use crate::components::top_toolbar::TopToolbar;
use crate::editor_state::{EditorContext, EditorState};
use yew::prelude::*;

#[function_component(App)]
pub fn app() -> Html {
    let editor = use_reducer(EditorState::default);

    html! {
        <ContextProvider<EditorContext> context={editor}>
            <div class="app-shell">
                <TopToolbar />
                <div class="app-shell__body">
                    <LeftPanel />
                    <CanvasArea />
                    <RightPanel />
                </div>
                <StatusBar />
            </div>
        </ContextProvider<EditorContext>>
    }
}
