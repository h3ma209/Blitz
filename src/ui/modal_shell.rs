//! Shared modal chrome: title row + ✕ close. Children = body / actions.

use dioxus::prelude::*;

#[component]
pub fn ModalShell(
    title: String,
    on_close: EventHandler<()>,
    #[props(default)] class: String,
    children: Element,
) -> Element {
    let modal_class = if class.is_empty() {
        "modal".to_string()
    } else {
        format!("modal {class}")
    };
    rsx! {
        div {
            class: "modal-backdrop",
            onclick: move |_| on_close.call(()),
            div {
                class: "{modal_class}",
                onclick: move |evt| evt.stop_propagation(),
                div { class: "modal-header",
                    h3 { "{title}" }
                    button {
                        class: "modal-close",
                        r#type: "button",
                        title: "Close",
                        aria_label: "Close",
                        onclick: move |_| on_close.call(()),
                        "✕"
                    }
                }
                {children}
            }
        }
    }
}
