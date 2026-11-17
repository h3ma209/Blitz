//! Shared empty-state primitive (Astryx pattern: consistent icon + title + hint + actions).

use dioxus::prelude::*;

use super::icons::ActionIcon;

/// Centered empty state with optional icon, heading, description, and action slot.
#[component]
pub fn EmptyState(
    title: String,
    #[props(default)] description: String,
    #[props(default)] icon: &'static str,
    #[props(default)] class: String,
    #[props(default)] children: Element,
) -> Element {
    let wrap_class = if class.is_empty() {
        "empty-state".to_string()
    } else {
        format!("empty-state {class}")
    };
    rsx! {
        div { class: "{wrap_class}",
            if !icon.is_empty() {
                span { class: "empty-state-icon",
                    ActionIcon { name: icon, size: 28 }
                }
            }
            h3 { class: "empty-state-title", "{title}" }
            if !description.is_empty() {
                p { class: "empty-state-desc hint", "{description}" }
            }
            {children}
        }
    }
}
