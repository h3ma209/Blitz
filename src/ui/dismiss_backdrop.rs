//! Transparent full-viewport layer that closes floating UI on pointer down outside.

use dioxus::prelude::*;

/// Renders a fixed, pointer-catching layer used to dismiss menus, popovers, and context UI.
///
/// @param on_dismiss - Invoked when the user clicks or context-clicks the backdrop.
/// @param z_index - Stacking order for the layer (dropdowns should sit above this value).
/// @param class - Optional extra CSS classes appended to `dismiss-backdrop`.
/// @returns A single full-screen `div` that forwards dismiss to `on_dismiss`.
#[component]
pub fn DismissBackdrop(
    on_dismiss: EventHandler<()>,
    #[props(default = 900)] z_index: i32,
    #[props(default)] class: String,
) -> Element {
    let extra = if class.is_empty() {
        String::new()
    } else {
        format!(" {class}")
    };
    let style = format!("z-index: {z_index};");

    rsx! {
        div {
            class: "dismiss-backdrop{extra}",
            style: "{style}",
            onclick: move |_| on_dismiss.call(()),
            oncontextmenu: move |evt| {
                evt.prevent_default();
                on_dismiss.call(());
            },
        }
    }
}
