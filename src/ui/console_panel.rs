use dioxus::prelude::*;

use crate::core::{ConsoleLevel, ConsoleLine};

#[component]
pub fn ConsolePanel(
    open: bool,
    lines: Vec<ConsoleLine>,
    resizing: bool,
    on_toggle: EventHandler<()>,
    on_clear: EventHandler<()>,
    on_resize_start: EventHandler<f64>,
) -> Element {
    rsx! {
        div { class: "console-wrap",
            div {
                class: if resizing {
                    "console-resize active"
                } else {
                    "console-resize"
                },
                title: "Drag to resize console",
                onmousedown: move |evt| {
                    evt.prevent_default();
                    evt.stop_propagation();
                    on_resize_start.call(evt.client_coordinates().y);
                },
            }
            div { class: "console-bar",
                button {
                    class: "builder-tab",
                    onclick: move |_| on_toggle.call(()),
                    if open { "Console ▾" } else { "Console ▸" }
                }
                if open {
                    button {
                        class: "btn-icon",
                        title: "Clear",
                        onclick: move |_| on_clear.call(()),
                        "Clear"
                    }
                }
            }
            if open {
                pre { class: "console-body",
                    if lines.is_empty() {
                        span { class: "hint", "Send a request — URL, status, script messages appear here." }
                    } else {
                        for line in lines.iter() {
                            {
                                let cls = match line.level {
                                    ConsoleLevel::Error => "console-err",
                                    ConsoleLevel::Warn => "console-warn",
                                    ConsoleLevel::Script => "console-script",
                                    ConsoleLevel::Info => "console-info",
                                };
                                let ts = line.at.format("%H:%M:%S").to_string();
                                rsx! {
                                    div { class: "{cls}",
                                        span { class: "console-ts", "[{ts}] " }
                                        "{line.message}"
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
