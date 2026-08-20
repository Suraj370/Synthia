use yew::prelude::*;

#[function_component(TopToolbar)]
pub fn top_toolbar() -> Html {
    html! {
        <header class="toolbar">
            <div class="toolbar__section toolbar__section--brand">
                <span class="toolbar__logo">{"◆"}</span>
                <span class="toolbar__title">{"Apollo"}</span>
            </div>

            <div class="toolbar__section toolbar__section--actions">
                <button class="toolbar__button" title="New">{"New"}</button>
                <button class="toolbar__button" title="Open">{"Open"}</button>
                <button class="toolbar__button" title="Save">{"Save"}</button>
            </div>

            <div class="toolbar__section toolbar__section--doc">
                <span class="toolbar__doc-name">{"Untitled"}</span>
            </div>

            <div class="toolbar__section toolbar__section--view">
                <button class="toolbar__button" title="Zoom out">{"−"}</button>
                <span class="toolbar__zoom">{"100%"}</span>
                <button class="toolbar__button" title="Zoom in">{"+"}</button>
            </div>
        </header>
    }
}
