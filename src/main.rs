//! ⚡ Blitz — local-first API client (Dioxus desktop).
//! Architecture mirrors design docs: requester workbench UI → runtime send pipeline.
//! CLI: `blitz run collection.json [--env env.json]`

use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;

mod brand;
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

    // Optional `--session <name>` / `--session=<name>` opens an isolated Blitz window
    // with its own cookies, auth tokens, env current_values, and WS sessions.
    let mut i = 1usize;
    while i < args.len() {
        if args[i] == "--session" {
            if let Some(name) = args.get(i + 1) {
                if !name.trim().is_empty() {
                    std::env::set_var("BLITZ_SESSION", name.trim());
                }
                i += 2;
                continue;
            }
        } else if let Some(name) = args[i].strip_prefix("--session=") {
            if !name.trim().is_empty() {
                std::env::set_var("BLITZ_SESSION", name.trim());
            }
        }
        i += 1;
    }

    let window_title = match storage::JsonStore::active_session_name() {
        Some(sess) => format!("⚡ Blitz · {sess}"),
        None => "⚡ Blitz".to_string(),
    };

    // Title + CSS must not rely on Dioxus.toml / `asset!` alone: those need `dx`
    // (or CARGO_MANIFEST_DIR). Plain cargo + Finder-launched .app gets "Dioxus App"
    // and broken stylesheet URLs otherwise.
    let cfg = Config::new()
        .with_window(
            WindowBuilder::new()
                .with_title(window_title)
                .with_inner_size(dioxus::desktop::tao::dpi::LogicalSize::new(1280.0, 820.0))
                .with_min_inner_size(dioxus::desktop::tao::dpi::LogicalSize::new(840.0, 560.0))
        )
        .with_custom_head(format!("<style>{}</style>", brand::STYLES_CSS.as_str()));

    dioxus::LaunchBuilder::desktop()
        .with_cfg(cfg)
        .launch(App);
}

#[component]
fn App() -> Element {
    let title = match storage::JsonStore::active_session_name() {
        Some(sess) => format!("⚡ Blitz · {sess}"),
        None => "⚡ Blitz".to_string(),
    };
    rsx! {
        document::Title { "{title}" }
        Workbench {}
    }
}
