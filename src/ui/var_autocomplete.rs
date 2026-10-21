//! `{{` variable insertion dropdown for editors and text fields.

use dioxus::prelude::*;

use crate::core::{var_insert_context, VariableContext, VariableSuggestion};

/// Active variable-insert menu anchored to the field.
#[derive(Debug, Clone, PartialEq)]
pub struct VarSuggestMenu {
    pub open_at: usize,
    pub filter: String,
    pub items: Vec<VariableSuggestion>,
    pub selected: usize,
    pub left: f64,
    pub top: f64,
}

impl VarSuggestMenu {
    /// Rebuild suggestions from the current buffer + caret.
    pub fn refresh(text: &str, caret: usize, vars: &VariableContext, left: f64, top: f64) -> Option<Self> {
        let (open_at, filter) = var_insert_context(text, caret)?;
        let items = vars.insert_suggestions(&filter);
        if items.is_empty() {
            return Some(Self {
                open_at,
                filter,
                items,
                selected: 0,
                left,
                top,
            });
        }
        Some(Self {
            open_at,
            filter,
            items,
            selected: 0,
            left,
            top,
        })
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let len = self.items.len();
        let next = (self.selected as isize + delta).rem_euclid(len as isize);
        self.selected = next as usize;
    }

    pub fn pick(&self) -> Option<&VariableSuggestion> {
        self.items.get(self.selected)
    }
}

/// Render the floating suggestion list (position: absolute within `.var-hover-host` / editor host).
#[component]
pub fn VarSuggestDropdown(
    menu: VarSuggestMenu,
    on_pick: EventHandler<VariableSuggestion>,
    on_dismiss: EventHandler<()>,
) -> Element {
    if menu.items.is_empty() {
        return rsx! {
            div {
                class: "var-suggest-menu",
                style: "left: {menu.left}px; top: {menu.top}px;",
                div { class: "var-suggest-empty", "No matching variables" }
            }
        };
    }

    rsx! {
        div {
            class: "var-suggest-menu",
            style: "left: {menu.left}px; top: {menu.top}px;",
            role: "listbox",
            onmousedown: move |evt| evt.prevent_default(),
            for (idx, item) in menu.items.iter().enumerate() {
                button {
                    class: if idx == menu.selected { "var-suggest-item active" } else { "var-suggest-item" },
                    r#type: "button",
                    role: "option",
                    onclick: {
                        let item = item.clone();
                        move |_| on_pick.call(item.clone())
                    },
                    span { class: "var-suggest-label", "{item.label}" }
                    span { class: "var-suggest-scope", "{item.scope.label()}" }
                }
            }
        }
    }
}

/// Read caret byte index and anchor position for a field by id (`input` or `textarea`).
pub async fn read_field_caret_anchor(element_id: &str) -> Option<(usize, f64, f64)> {
    let script = format!(
        r#"
        const el = document.getElementById({id});
        if (!el) return null;
        const rect = el.getBoundingClientRect();
        const host = el.closest('.var-hover-host, .code-editor-host');
        const hr = host ? host.getBoundingClientRect() : rect;
        const val = el.value || '';
        const caretU16 = el.selectionStart ?? 0;
        // Convert UTF-16 code units to UTF-8 byte offset for safe Rust string slicing
        const sub = val.slice(0, caretU16);
        const byteLen = new TextEncoder().encode(sub).length;
        return [byteLen, 8, rect.bottom - hr.top + 4];
        "#,
        id = serde_json::to_string(element_id).unwrap_or_else(|_| "\"\"".into())
    );
    let v = document::eval(&script).await.ok()?;
    parse_caret_anchor(&v)
}

/// Move caret after Rust state updated the controlled field value.
/// Accepts UTF-8 byte offset and maps it to UTF-16 code unit offset for `setSelectionRange`.
pub async fn restore_field_caret(element_id: &str, byte_caret: usize) {
    let script = format!(
        r#"
        await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
        const el = document.getElementById({id});
        if (!el) return false;
        el.focus();
        try {{
            const targetBytes = {byte_caret};
            const val = el.value || '';
            const enc = new TextEncoder();
            let u16Pos = 0;
            let currentBytes = 0;
            for (const ch of val) {{
                const chBytes = enc.encode(ch).length;
                if (currentBytes + chBytes > targetBytes) break;
                currentBytes += chBytes;
                u16Pos += ch.length;
            }}
            el.setSelectionRange(u16Pos, u16Pos);
        }} catch (_) {{}}
        return true;
        "#,
        id = serde_json::to_string(element_id).unwrap_or_else(|_| "\"\"".into()),
        byte_caret = byte_caret
    );
    let _ = document::eval(&script).await;
}

fn parse_caret_anchor(v: &serde_json::Value) -> Option<(usize, f64, f64)> {
    let arr = v.as_array()?;
    let idx = arr.first()?.as_u64()? as usize;
    let left = arr
        .get(1)?
        .as_f64()
        .or_else(|| arr.get(1)?.as_u64().map(|n| n as f64))?;
    let top = arr
        .get(2)?
        .as_f64()
        .or_else(|| arr.get(2)?.as_u64().map(|n| n as f64))?;
    Some((idx, left, top))
}
