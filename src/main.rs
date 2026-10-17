//! notPostman — local-first API client (Dioxus desktop).
//! Architecture mirrors reports/: requester workbench UI → runtime send pipeline.

use dioxus::prelude::*;

mod core;
mod runtime;
mod secrets;
mod storage;
mod ui;

#[cfg(test)]
mod ui_flow_smoke;

use ui::Workbench;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Stylesheet { href: asset!("/assets/styles.css") }
        Workbench {}
    }
}
