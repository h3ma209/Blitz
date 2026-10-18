use dioxus::prelude::*;

use crate::core::UiTheme;

/// Serialize theme for `<select value>`.
fn theme_value(theme: UiTheme) -> &'static str {
    match theme {
        UiTheme::Spotify => "spotify",
        UiTheme::Slate => "slate",
        UiTheme::Light => "light",
        UiTheme::HighContrast => "high_contrast",
    }
}

/// Compact theme picker for the top bar (applies immediately via parent).
#[component]
pub fn ThemeSelector(theme: UiTheme, on_change: EventHandler<UiTheme>) -> Element {
    let value = theme_value(theme);

    rsx! {
        label {
            class: "theme-picker",
            title: "Color theme",
            span { class: "theme-picker-label type-caption", "Theme" }
            select {
                class: "theme-select",
                value: "{value}",
                aria_label: "Color theme",
                onchange: move |evt: Event<FormData>| {
                    let next = match evt.value().as_str() {
                        "slate" => UiTheme::Slate,
                        "light" => UiTheme::Light,
                        "high_contrast" => UiTheme::HighContrast,
                        _ => UiTheme::Spotify,
                    };
                    on_change.call(next);
                },
                for t in UiTheme::all() {
                    option {
                        value: "{theme_value(*t)}",
                        "{t.label()}"
                    }
                }
            }
        }
    }
}
