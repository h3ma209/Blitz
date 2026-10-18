//! Multiline text areas — delegates to [`code_editor::MultilineCodeField`].

use dioxus::prelude::*;

use crate::core::VariableContext;

use super::code_editor::{EditorSyntax, MultilineCodeField};

/// Shared multiline editor with ⌘/ line + ⌘⇧/ block comment toggles.
/// When `vars` is set, `{{…}}` tokens get a highlight mirror and `{{` insertion menu.
#[component]
pub fn CommentableTextarea(
    class: String,
    value: String,
    on_change: EventHandler<String>,
    #[props(default)] placeholder: String,
    #[props(default)] rows: Option<&'static str>,
    #[props(default)] vars: Option<VariableContext>,
) -> Element {
    rsx! {
        MultilineCodeField {
            class,
            value,
            on_change,
            placeholder,
            rows,
            vars,
            syntax: EditorSyntax::Plain,
            js_completions: false,
        }
    }
}
