//! Compile-time embedded brand assets.
//!
//! Dioxus `asset!` does **not** bake file bytes into the binary — `dx` must rewrite
//! `bundled_path` metadata. Plain `cargo build` + Finder launch sets
//! `is_bundled_app() == true` with a placeholder path, so CSS/logo URLs 404.
//! Embedding via `include_str!` / `include_bytes!` works for both `cargo run` and
//! installed `.app` bundles.

use std::sync::LazyLock;

use base64::Engine;

const RAW_CSS: &str = include_str!("../assets/styles.css");
static INTER_WOFF2: &[u8] = include_bytes!("../assets/fonts/Inter-Variable.woff2");
static JBM_WOFF2: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Variable.woff2");

/// Full CSS embedded at compile time with offline `@font-face` data URIs.
pub static STYLES_CSS: LazyLock<String> = LazyLock::new(|| {
    let b64 = base64::engine::general_purpose::STANDARD;
    let face = |fam: &str, bytes: &[u8]| {
        format!(
            "@font-face{{font-family:\"{fam}\";src:url(data:font/woff2;base64,{}) format(\"woff2\");font-weight:100 900;font-style:normal;font-display:block;}}",
            b64.encode(bytes)
        )
    };
    let fonts_css = format!("{}\n{}", face("Inter", INTER_WOFF2), face("JetBrains Mono", JBM_WOFF2));
    RAW_CSS.replacen("/*@FONTS@*/", &fonts_css, 1)
});

/// `data:` URI for the brand logo — no runtime filesystem lookup.
/// Rendered directly in desktop UI (top bar, splash screen, env switch overlay).
pub static LOGO_SRC: LazyLock<String> = LazyLock::new(|| {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(include_bytes!("../assets/logo.png"))
    )
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_css_embedded_and_nonempty() {
        assert!(STYLES_CSS.len() > 100, "expected styles.css baked in");
        assert!(
            STYLES_CSS.contains("top-bar") || STYLES_CSS.contains("brand"),
            "expected workbench selectors in embedded CSS"
        );
        assert!(STYLES_CSS.contains("@font-face"), "expected embedded @font-face rules");
        assert!(!STYLES_CSS.contains("fonts.googleapis.com"), "must not fetch Google Fonts online");
    }

    #[test]
    fn logo_data_uri() {
        let src = LOGO_SRC.as_str();
        assert!(src.starts_with("data:image/png;base64,"));
        assert!(src.len() > 64);
    }
}
