//! Shared loading chrome: spinner + skeleton placeholders + full overlay.
//!
//! Used wherever the app shows async activity (Send, import/sync, runner,
//! perf, env switch) so every "loading" looks like a loading.

use dioxus::prelude::*;

/// A compact spinning loader (14px by default). Pair with a label via
/// [`LoadingRow`] / [`LoadingOverlay`].
#[component]
pub fn Spinner(
    /// Optional extra class applied to the wrapper.
    #[props(default)] class: String,
) -> Element {
    let cls = if class.is_empty() {
        "loading-spinner".to_string()
    } else {
        format!("loading-spinner {class}")
    };
    rsx! {
        span { class: "{cls}" }
    }
}

/// A single-line loading row: spinner + optional text.
#[component]
pub fn LoadingRow(
    text: String,
    #[props(default)] class: String,
) -> Element {
    let cls = if class.is_empty() {
        "loading-row".to_string()
    } else {
        format!("loading-row {class}")
    };
    rsx! {
        div { class: "{cls}",
            Spinner { }
            span { class: "loading-text", "{text}" }
        }
    }
}

/// Skeleton placeholder bars for content that is being fetched (e.g. the
/// response body while sending). `n` bars are rendered.
#[component]
pub fn SkeletonBars(
    n: usize,
    #[props(default)] class: String,
) -> Element {
    let cls = if class.is_empty() {
        "skeleton-bars".to_string()
    } else {
        format!("skeleton-bars {class}")
    };
    rsx! {
        div { class: "{cls}",
            for i in 0..n {
                div { class: "skeleton-bar", key: "{i}" }
            }
        }
    }
}

/// Full-pane loading overlay — spinner centered over the given content.
/// Used for the response pane while sending and for async tool panels.
#[component]
pub fn LoadingOverlay(
    text: String,
    children: Element,
    #[props(default)] show: bool,
    #[props(default)] subtle: bool,
) -> Element {
    let overlay_cls = if subtle {
        "loading-overlay loading-overlay-subtle"
    } else {
        "loading-overlay"
    };
    rsx! {
        div { class: "loading-overlay-wrap",
            {children}
            if show {
                div { class: "{overlay_cls}",
                    Spinner { }
                    if !text.is_empty() {
                        span { class: "loading-text", "{text}" }
                    }
                }
            }
        }
    }
}