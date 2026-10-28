//! Keyboard shortcut labels and handlers for the collections sidebar (Postman-style).

use dioxus::prelude::*;

/// Modifier prefix for shortcut display (`⌘` on macOS, `Ctrl+` elsewhere).
pub fn mod_prefix() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "⌘"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "Ctrl+"
    }
}

/// Whether the event uses the platform command modifier (⌘ or Ctrl).
pub fn meta_or_ctrl(evt: &Event<KeyboardData>) -> bool {
    evt.modifiers().meta() || evt.modifiers().ctrl()
}

/// Display string for duplicate (⌘D / Ctrl+D).
pub fn label_duplicate() -> String {
    format!("{}D", mod_prefix())
}

/// Display string for delete shortcut hint (⌘Delete on macOS, Ctrl+Delete elsewhere).
///
/// Matches [`tree_key_action`] delete bindings: plain Delete, ⌘/Ctrl+Backspace, and ⌘/Ctrl+Delete.
pub fn label_delete() -> String {
    #[cfg(target_os = "macos")]
    {
        "⌘Delete".to_string()
    }
    #[cfg(not(target_os = "macos"))]
    {
        "Ctrl+Delete".to_string()
    }
}

/// Display string for rename (F2).
pub fn label_rename() -> &'static str {
    "F2"
}

/// Display string for copy (⌘C / Ctrl+C).
pub fn label_copy() -> String {
    format!("{}C", mod_prefix())
}

/// Display string for cut (⌘X / Ctrl+X).
pub fn label_cut() -> String {
    format!("{}X", mod_prefix())
}

/// Display string for paste (⌘V / Ctrl+V).
pub fn label_paste() -> String {
    format!("{}V", mod_prefix())
}

/// Display string for open request (↵).
pub fn label_open() -> &'static str {
    "↵"
}

/// Context menu row with optional shortcut hint on the right.
#[component]
pub fn CtxMenuItem(
    label: String,
    #[props(default)] shortcut: Option<String>,
    #[props(default = false)] danger: bool,
    onclick: EventHandler<()>,
) -> Element {
    let class = if danger {
        "ctx-item danger"
    } else {
        "ctx-item"
    };
    rsx! {
        button {
            class: "{class}",
            onclick: move |_| onclick.call(()),
            span { class: "ctx-item-label", "{label}" }
            if let Some(sc) = shortcut {
                span { class: "ctx-item-shortcut", "{sc}" }
            }
        }
    }
}

/// Postman-like tree shortcuts when the sidebar tree has focus and a row is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeKeyAction {
    Delete,
    Duplicate,
    Rename,
    Copy,
    Cut,
    Paste,
    Open,
    NavUp,
    NavDown,
    NavLeft,
    NavRight,
}

/// Map a keydown on the tree to an action, if any.
///
/// Delete accepts: unmodified Delete, platform meta+Backspace, and meta+Delete (⌘ or Ctrl).
pub fn tree_key_action(evt: &Event<KeyboardData>) -> Option<TreeKeyAction> {
    let meta = meta_or_ctrl(evt);
    let shift = evt.modifiers().shift();
    match evt.key() {
        Key::Enter => Some(TreeKeyAction::Open),
        Key::F2 => Some(TreeKeyAction::Rename),
        Key::ArrowUp => Some(TreeKeyAction::NavUp),
        Key::ArrowDown => Some(TreeKeyAction::NavDown),
        Key::ArrowLeft => Some(TreeKeyAction::NavLeft),
        Key::ArrowRight => Some(TreeKeyAction::NavRight),
        Key::Backspace if meta => Some(TreeKeyAction::Delete),
        Key::Delete if meta => Some(TreeKeyAction::Delete),
        Key::Delete if !meta && !shift => Some(TreeKeyAction::Delete),
        Key::Character(c) if meta && !shift && c.eq_ignore_ascii_case("d") => {
            Some(TreeKeyAction::Duplicate)
        }
        Key::Character(c) if meta && !shift && c.eq_ignore_ascii_case("c") => {
            Some(TreeKeyAction::Copy)
        }
        Key::Character(c) if meta && !shift && c.eq_ignore_ascii_case("x") => {
            Some(TreeKeyAction::Cut)
        }
        Key::Character(c) if meta && !shift && c.eq_ignore_ascii_case("v") => {
            Some(TreeKeyAction::Paste)
        }
        _ => None,
    }
}
