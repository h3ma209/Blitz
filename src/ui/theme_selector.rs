use dioxus::prelude::*;

use crate::core::UiTheme;
use super::DismissBackdrop;

/// Compact theme icon button with popup picker for the top bar.
#[component]
pub fn ThemeSelector(theme: UiTheme, on_change: EventHandler<UiTheme>) -> Element {
    let mut menu_open = use_signal(|| false);

    rsx! {
        div { class: "theme-selector-wrap",
            if menu_open() {
                DismissBackdrop {
                    z_index: 900,
                    on_dismiss: move |_| menu_open.set(false),
                }
            }
            button {
                class: if menu_open() { "top-icon top-icon-on" } else { "top-icon" },
                title: format!("Theme: {}", theme.label()),
                aria_label: "Theme",
                onclick: move |_| menu_open.set(!menu_open()),
                span { class: "action-icon action-icon-theme", aria_hidden: "true" }
            }
            if menu_open() {
                div { class: "theme-menu",
                    div { class: "theme-menu-header", "Color Theme" }
                    for t in UiTheme::all() {
                        {
                            let item_theme = *t;
                            let is_active = item_theme == theme;
                            let row_class = if is_active {
                                "theme-menu-row theme-menu-row-active"
                            } else {
                                "theme-menu-row"
                            };
                            rsx! {
                                button {
                                    key: "{item_theme.class_token()}",
                                    class: "{row_class}",
                                    onclick: move |_| {
                                        menu_open.set(false);
                                        on_change.call(item_theme);
                                    },
                                    span { class: "theme-menu-dot theme-dot-{item_theme.class_token()}" }
                                    span { class: "theme-menu-label", "{item_theme.label()}" }
                                    if is_active {
                                        span { class: "theme-menu-check", "✓" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
