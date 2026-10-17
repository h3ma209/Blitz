//! notPostman — local-first API client (Dioxus desktop).
//! Architecture mirrors reports/: requester workbench UI → runtime send pipeline.
//! CLI: `not_postman run collection.json [--env env.json]`

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
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = core::cli_runner::try_run_cli(&args) {
        std::process::exit(code);
    }
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Stylesheet { href: asset!("/assets/styles.css") }
        Workbench {}
    }
}
