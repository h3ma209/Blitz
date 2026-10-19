//! Multiline code fields: syntax mirror, `{{` variable menu, Blitz `blitz.*` completions.

use dioxus::prelude::*;

use crate::core::{apply_var_insert, var_insert_context, VariableContext, VariableSuggestion};

use super::comment_toggle::{
    is_toggle_block_comment_shortcut, is_toggle_line_comment_shortcut, toggle_block_comments,
    toggle_line_comments,
};
use super::js_highlight::{render_js_var_mirror_html, render_var_mirror_html};
use super::var_autocomplete::{
    read_field_caret_anchor, restore_field_caret, VarSuggestDropdown, VarSuggestMenu,
};

/// Syntax highlighting mode for editor mirrors.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorSyntax {
    #[default]
    Plain,
    JavaScript,
    Json,
}

/// Blitz sandbox API snippets & helpers for script editor autocomplete.
const BLITZ_COMPLETIONS: &[(&str, &str)] = &[
    // Helper Test Snippets
    ("blitz.test: Status code 200", "blitz.test(\"Status is 200\", function () {\n  blitz.response.to.have.status(200);\n});"),
    ("blitz.test: Response time < 200ms", "blitz.test(\"Response time < 200ms\", function () {\n  blitz.expect(blitz.response.responseTime).to.be.below(200);\n});"),
    ("blitz.test: JSON property check", "blitz.test(\"Has JSON property\", function () {\n  var json = blitz.response.json();\n  blitz.expect(json).to.have.property(\"id\");\n});"),
    ("blitz.test: JSON value check", "blitz.test(\"Check value\", function () {\n  var json = blitz.response.json();\n  blitz.expect(json.status).to.equal(\"success\");\n});"),
    ("blitz.test: Content-Type header", "blitz.test(\"Content-Type is JSON\", function () {\n  blitz.expect(blitz.response.headers.get(\"content-type\")).to.include(\"application/json\");\n});"),
    ("blitz.test: Generic test", "blitz.test(\"test name\", function () {\n  \n});"),

    // Expect Assertions
    ("blitz.expect", "blitz.expect("),
    ("blitz.expect.to.equal", "blitz.expect(actual).to.equal(expected);"),
    ("blitz.expect.to.eql", "blitz.expect(actual).to.eql(expected);"),
    ("blitz.expect.to.include", "blitz.expect(actual).to.include(part);"),
    ("blitz.expect.to.be.ok", "blitz.expect(actual).to.be.ok;"),
    ("blitz.expect.to.be.above", "blitz.expect(actual).to.be.above(0);"),
    ("blitz.expect.to.be.below", "blitz.expect(actual).to.be.below(200);"),
    ("blitz.expect.to.be.a", "blitz.expect(actual).to.be.a(\"string\");"),
    ("blitz.expect.to.have.property", "blitz.expect(actual).to.have.property(\"key\");"),
    ("blitz.expect.to.not.equal", "blitz.expect(actual).to.not.equal(expected);"),

    // Environment Variables
    ("blitz.environment.set", "blitz.environment.set(\"key\", \"value\");"),
    ("blitz.environment.get", "blitz.environment.get(\"key\");"),
    ("blitz.environment.unset", "blitz.environment.unset(\"key\");"),
    ("blitz.environment.has", "blitz.environment.has(\"key\");"),
    ("blitz.environment.toObject", "blitz.environment.toObject();"),
    ("blitz.environment.clear", "blitz.environment.clear();"),

    // Collection Variables
    ("blitz.collectionVariables.set", "blitz.collectionVariables.set(\"key\", \"value\");"),
    ("blitz.collectionVariables.get", "blitz.collectionVariables.get(\"key\");"),
    ("blitz.collectionVariables.unset", "blitz.collectionVariables.unset(\"key\");"),
    ("blitz.collectionVariables.clear", "blitz.collectionVariables.clear();"),

    // Global Variables
    ("blitz.globals.set", "blitz.globals.set(\"key\", \"value\");"),
    ("blitz.globals.get", "blitz.globals.get(\"key\");"),
    ("blitz.globals.unset", "blitz.globals.unset(\"key\");"),
    ("blitz.globals.clear", "blitz.globals.clear();"),

    // Combined Variables
    ("blitz.variables.get", "blitz.variables.get(\"key\");"),
    ("blitz.variables.set", "blitz.variables.set(\"key\", \"value\");"),
    ("blitz.variables.replaceIn", "blitz.variables.replaceIn(\"{{var}}\");"),

    // Secrets / Vault
    ("blitz.vault.get", "blitz.vault.get(\"key\");"),
    ("blitz.vault.set", "blitz.vault.set(\"key\", \"value\");"),
    ("blitz.vault.unset", "blitz.vault.unset(\"key\");"),

    // Cookies
    ("blitz.cookies.get", "blitz.cookies.get(\"key\");"),
    ("blitz.cookies.set", "blitz.cookies.set(\"key\", \"value\");"),
    ("blitz.cookies.clear", "blitz.cookies.clear();"),

    // Response Helpers
    ("blitz.response.to.have.status", "blitz.response.to.have.status(200);"),
    ("blitz.response.to.have.jsonBody", "blitz.response.to.have.jsonBody();"),
    ("blitz.response.json", "blitz.response.json()"),
    ("blitz.response.text", "blitz.response.text()"),
    ("blitz.response.code", "blitz.response.code"),
    ("blitz.response.status", "blitz.response.status"),
    ("blitz.response.responseTime", "blitz.response.responseTime"),
    ("blitz.response.headers.get", "blitz.response.headers.get(\"content-type\")"),

    // Request Helpers
    ("blitz.request.headers.upsert", "blitz.request.headers.upsert({ key: \"Header-Key\", value: \"Header-Value\" });"),
    ("blitz.request.headers.get", "blitz.request.headers.get(\"Header-Key\")"),
    ("blitz.request.headers.remove", "blitz.request.headers.remove(\"Header-Key\");"),
    ("blitz.request.url.getPath", "blitz.request.url.getPath()"),
    ("blitz.request.method", "blitz.request.method"),
    ("blitz.request.body.raw", "blitz.request.body.raw"),

    // Execution & HTTP
    ("blitz.sendRequest", "blitz.sendRequest(url, function (err, res) {\n  \n});"),
    ("blitz.execution.setNextRequest", "blitz.execution.setNextRequest(\"Request name\");"),
    ("blitz.execution.skipRequest", "blitz.execution.skipRequest();"),
    ("blitz.visualizer.set", "blitz.visualizer.set(\"<div>{{data}}</div>\", data);"),
    ("blitz.info.eventName", "blitz.info.eventName"),
    ("blitz.info.requestName", "blitz.info.requestName"),

    // Utilities & Console
    ("console.log", "console.log("),
    ("console.warn", "console.warn("),
    ("console.error", "console.error("),
    ("CryptoJS.SHA256", "CryptoJS.SHA256(\"text\").toString();"),
    ("CryptoJS.HmacSHA256", "CryptoJS.HmacSHA256(\"message\", \"secret\").toString();"),
];

#[derive(Debug, Clone, PartialEq)]
struct JsSuggestMenu {
    replace_start: usize,
    items: Vec<(String, String)>,
    selected: usize,
    left: f64,
    top: f64,
}

/// Word prefix before `caret` for JS identifier completion.
fn js_complete_prefix(text: &str, caret: usize) -> Option<(usize, String)> {
    let caret = crate::core::snap_to_char_boundary(text, caret);
    let before = &text[..caret];
    let mut start = caret;
    while start > 0 {
        let prev = crate::core::snap_to_char_boundary(before, start - 1);
        let ch = before.as_bytes()[start - 1];
        if ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'$' || ch == b'.' {
            start = prev;
        } else {
            break;
        }
    }
    if start == caret {
        return None;
    }
    let prefix = before[start..caret].to_string();
    if prefix.len() < 2 {
        return None;
    }
    Some((start, prefix))
}

fn js_suggestions(prefix: &str) -> Vec<(String, String)> {
    let lower = prefix.to_lowercase();
    // Normalize alias if user starts typing blitz. or pm.
    let normalized = if let Some(rest) = lower.strip_prefix("blitz.") {
        format!("blitz.{rest}")
    } else if let Some(rest) = lower.strip_prefix("pm.") {
        format!("blitz.{rest}")
    } else if lower == "epm" || lower == "pm" {
        "blitz".to_string()
    } else {
        lower.clone()
    };

    BLITZ_COMPLETIONS
        .iter()
        .filter(|(label, _)| {
            let l = label.to_lowercase();
            l.starts_with(&lower) || l.starts_with(&normalized)
        })
        .map(|(label, snippet)| (label.to_string(), snippet.to_string()))
        .take(16)
        .collect()
}

fn apply_js_insert(text: &str, caret: usize, start: usize, snippet: &str) -> (String, usize) {
    let start = crate::core::snap_to_char_boundary(text, start);
    let caret = crate::core::snap_to_char_boundary(text, caret.max(start));
    let mut out = String::with_capacity(text.len() + snippet.len());
    out.push_str(&text[..start]);
    out.push_str(snippet);
    out.push_str(&text[caret..]);
    let new_caret = start + snippet.len();
    (out, new_caret)
}

/// Multiline editor with optional JS highlighting, `{{` variables, and `blitz.*` completions.
#[component]
pub fn MultilineCodeField(
    class: String,
    value: String,
    on_change: EventHandler<String>,
    #[props(default)] placeholder: String,
    #[props(default)] rows: Option<&'static str>,
    #[props(default)] vars: Option<VariableContext>,
    #[props(default)] syntax: EditorSyntax,
    #[props(default)] js_completions: bool,
) -> Element {
    let value_for_fallback = value.clone();
    let rows_attr = rows.unwrap_or("");
    let field_id = use_signal(|| format!("np-code-{}", uuid::Uuid::new_v4()));
    let mut var_menu = use_signal(|| None::<VarSuggestMenu>);
    let mut js_menu = use_signal(|| None::<JsSuggestMenu>);

    let highlight =
        vars.is_some() || syntax == EditorSyntax::JavaScript || syntax == EditorSyntax::Json;
    let mirror_html = match (syntax, vars.as_ref()) {
        (EditorSyntax::JavaScript, v) => render_js_var_mirror_html(&value, v),
        (EditorSyntax::Json, v) => super::json_highlight::render_json_var_mirror_html(&value, v),
        (_, Some(v)) => render_var_mirror_html(&value, v),
        _ => String::new(),
    };
    let ta_class = if highlight {
        format!("{class} var-highlight-input")
    } else {
        class.clone()
    };
    let host_class = "var-highlight-host var-highlight-host-multiline code-editor-host";

    let textarea = rsx! {
        textarea {
            id: "{field_id()}",
            class: "{ta_class}",
            value: "{value}",
            placeholder: "{placeholder}",
            rows: "{rows_attr}",
            oninput: {
                let value = value.clone();
                let vars = vars.clone();
                let field_id = field_id.clone();
                move |evt: Event<FormData>| {
                    on_change.call(evt.value());
                    let value = value.clone();
                    let vars = vars.clone();
                    let field_id = field_id();
                    spawn(async move {
                        refresh_editor_menus(
                            &value,
                            vars.as_ref(),
                            js_completions,
                            &field_id,
                            var_menu,
                            js_menu,
                        )
                        .await;
                    });
                }
            },
            onkeyup: {
                let value = value.clone();
                let vars = vars.clone();
                let field_id = field_id.clone();
                move |_| {
                    let value = value.clone();
                    let vars = vars.clone();
                    let field_id = field_id();
                    spawn(async move {
                        refresh_editor_menus(
                            &value,
                            vars.as_ref(),
                            js_completions,
                            &field_id,
                            var_menu,
                            js_menu,
                        )
                        .await;
                    });
                }
            },
            onclick: {
                let value = value.clone();
                let vars = vars.clone();
                let field_id = field_id.clone();
                move |_| {
                    let value = value.clone();
                    let vars = vars.clone();
                    let field_id = field_id();
                    spawn(async move {
                        refresh_editor_menus(
                            &value,
                            vars.as_ref(),
                            js_completions,
                            &field_id,
                            var_menu,
                            js_menu,
                        )
                        .await;
                    });
                }
            },
            onscroll: move |_| {
                if !highlight {
                    return;
                }
                let id = field_id();
                spawn(async move {
                    sync_code_mirror_scroll(&id).await;
                });
            },
            onkeydown: move |evt: Event<KeyboardData>| {
                if let Some(mut m) = var_menu() {
                    match evt.key() {
                        Key::ArrowDown => {
                            evt.prevent_default();
                            m.move_selection(1);
                            var_menu.set(Some(m));
                            return;
                        }
                        Key::ArrowUp => {
                            evt.prevent_default();
                            m.move_selection(-1);
                            var_menu.set(Some(m));
                            return;
                        }
                        Key::Escape => {
                            evt.prevent_default();
                            var_menu.set(None);
                            return;
                        }
                        Key::Enter | Key::Tab => {
                            if let Some(item) = m.pick() {
                                evt.prevent_default();
                                let item = item.clone();
                                let open = m.open_at;
                                let id = field_id();
                                let value = value_for_fallback.clone();
                                let on_change = on_change.clone();
                                spawn(async move {
                                    if let Some((caret, _, _)) = read_field_caret_anchor(&id).await {
                                        let (new_text, new_caret) =
                                            apply_var_insert(&value, caret, open, &item.insert);
                                        on_change.call(new_text);
                                        restore_field_caret(&id, new_caret).await;
                                    }
                                });
                                var_menu.set(None);
                            }
                            return;
                        }
                        _ => {}
                    }
                }

                if let Some(mut m) = js_menu() {
                    match evt.key() {
                        Key::ArrowDown => {
                            evt.prevent_default();
                            if !m.items.is_empty() {
                                let len = m.items.len();
                                m.selected = (m.selected + 1) % len;
                                js_menu.set(Some(m));
                            }
                            return;
                        }
                        Key::ArrowUp => {
                            evt.prevent_default();
                            if !m.items.is_empty() {
                                let len = m.items.len();
                                m.selected = (m.selected + len - 1) % len;
                                js_menu.set(Some(m));
                            }
                            return;
                        }
                        Key::Escape => {
                            evt.prevent_default();
                            js_menu.set(None);
                            return;
                        }
                        Key::Enter | Key::Tab => {
                            if let Some((_, snippet)) = m.items.get(m.selected) {
                                evt.prevent_default();
                                let start = m.replace_start;
                                let snippet = snippet.clone();
                                let id = field_id();
                                let value = value_for_fallback.clone();
                                let on_change = on_change.clone();
                                spawn(async move {
                                    if let Some((caret, _, _)) = read_field_caret_anchor(&id).await {
                                        let (new_text, new_caret) =
                                            apply_js_insert(&value, caret, start, &snippet);
                                        on_change.call(new_text.clone());
                                        restore_field_caret(&id, new_caret).await;
                                    }
                                });
                                js_menu.set(None);
                            }
                            return;
                        }
                        _ => {}
                    }
                }

                let line = is_toggle_line_comment_shortcut(&evt);
                let block = is_toggle_block_comment_shortcut(&evt);
                if !line && !block {
                    return;
                }
                evt.prevent_default();
                let fallback = value_for_fallback.clone();
                let on_change = on_change.clone();
                let id = field_id();
                spawn(async move {
                    let (text, start, end) = match read_textarea_state_by_id(&id).await {
                        Some(state) => state,
                        None => {
                            let len = fallback.len();
                            (fallback, len, len)
                        }
                    };
                    let (new_text, _new_start, new_end) = if block {
                        toggle_block_comments(&text, start, end)
                    } else {
                        toggle_line_comments(&text, start, end)
                    };
                    if new_text != text {
                        on_change.call(new_text.clone());
                    }
                    restore_field_caret(&id, new_end).await;
                });
            },
        }
    };

    rsx! {
        div {
            class: "{host_class}",
            if highlight {
                div {
                    class: "var-highlight-mirror var-highlight-mirror-multiline code-editor-mirror",
                    "aria-hidden": "true",
                    dangerous_inner_html: "{mirror_html}",
                }
            }
            {textarea}
            if let Some(menu) = var_menu() {
                if vars.is_some() {
                    VarSuggestDropdown {
                        menu: menu.clone(),
                        on_pick: {
                            let field_id = field_id.clone();
                            let value = value.clone();
                            let on_change = on_change.clone();
                            move |item: VariableSuggestion| {
                                let open = var_menu().map(|m| m.open_at).unwrap_or(0);
                                let id = field_id();
                                let value = value.clone();
                                let on_change = on_change.clone();
                                spawn(async move {
                                    if let Some((caret, _, _)) = read_field_caret_anchor(&id).await {
                                        let (new_text, new_caret) =
                                            apply_var_insert(&value, caret, open, &item.insert);
                                        on_change.call(new_text);
                                        restore_field_caret(&id, new_caret).await;
                                    }
                                });
                                var_menu.set(None);
                            }
                        },
                        on_dismiss: move |_| var_menu.set(None),
                    }
                }
            }
            if let Some(menu) = js_menu() {
                if !menu.items.is_empty() {
                    div {
                        class: "var-suggest-menu js-suggest-menu",
                        style: "left: {menu.left}px; top: {menu.top}px;",
                        onmousedown: move |evt| evt.prevent_default(),
                        for (idx, (label, _snippet)) in menu.items.iter().enumerate() {
                            button {
                                class: if idx == menu.selected { "var-suggest-item active" } else { "var-suggest-item" },
                                r#type: "button",
                                onclick: {
                                    let snippet = menu.items[idx].1.clone();
                                    let start = menu.replace_start;
                                    let field_id = field_id.clone();
                                    let value = value.clone();
                                    let on_change = on_change.clone();
                                    move |_| {
                                        let id = field_id();
                                        let value = value.clone();
                                        let on_change = on_change.clone();
                                        let snippet = snippet.clone();
                                        spawn(async move {
                                            if let Some((caret, _, _)) = read_field_caret_anchor(&id).await {
                                                let (new_text, new_caret) =
                                                    apply_js_insert(&value, caret, start, &snippet);
                                                on_change.call(new_text);
                                                restore_field_caret(&id, new_caret).await;
                                            }
                                        });
                                        js_menu.set(None);
                                    }
                                },
                                span { class: "var-suggest-label", "{label}" }
                                span { class: "var-suggest-scope", "pm" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Script panel editor (pre-request + tests).
#[component]
pub fn ScriptCodeEditor(
    class: String,
    value: String,
    on_change: EventHandler<String>,
    #[props(default)] placeholder: String,
    vars: VariableContext,
) -> Element {
    rsx! {
        MultilineCodeField {
            class,
            value,
            on_change,
            placeholder,
            vars: Some(vars),
            syntax: EditorSyntax::JavaScript,
            js_completions: true,
        }
    }
}

async fn sync_code_mirror_scroll(textarea_id: &str) {
    let script = format!(
        r#"
        const el = document.getElementById({id});
        if (!el) return false;
        const host = el.closest('.code-editor-host');
        const mirror = host && host.querySelector('.code-editor-mirror, .var-highlight-mirror');
        if (mirror) {{
            mirror.scrollLeft = el.scrollLeft || 0;
            mirror.scrollTop = el.scrollTop || 0;
        }}
        return true;
        "#,
        id = serde_json::to_string(textarea_id).unwrap_or_else(|_| "\"\"".into())
    );
    let _ = document::eval(&script).await;
}

async fn refresh_editor_menus(
    value: &str,
    vars: Option<&VariableContext>,
    js_completions: bool,
    field_id: &str,
    mut var_menu: Signal<Option<VarSuggestMenu>>,
    mut js_menu: Signal<Option<JsSuggestMenu>>,
) {
    if let Some((caret, left, top)) = read_field_caret_anchor(field_id).await {
        if let Some(v) = vars {
            if var_insert_context(value, caret).is_some() {
                var_menu.set(VarSuggestMenu::refresh(value, caret, v, left, top));
                js_menu.set(None);
                return;
            }
        }
        var_menu.set(None);
        if js_completions {
            if let Some((start, prefix)) = js_complete_prefix(value, caret) {
                let items = js_suggestions(&prefix);
                if !items.is_empty() {
                    js_menu.set(Some(JsSuggestMenu {
                        replace_start: start,
                        items,
                        selected: 0,
                        left,
                        top,
                    }));
                    return;
                }
            }
        }
        js_menu.set(None);
    }
}

async fn read_textarea_state_by_id(id: &str) -> Option<(String, usize, usize)> {
    let script = format!(
        r#"
        const el = document.getElementById({id});
        if (!el || el.tagName !== 'TEXTAREA') return null;
        const val = el.value || '';
        const enc = new TextEncoder();
        const startU16 = el.selectionStart ?? 0;
        const endU16 = el.selectionEnd ?? 0;
        const startBytes = enc.encode(val.slice(0, startU16)).length;
        const endBytes = enc.encode(val.slice(0, endU16)).length;
        return [val, startBytes, endBytes];
        "#,
        id = serde_json::to_string(id).unwrap_or_else(|_| "\"\"".into())
    );
    let v = document::eval(&script).await.ok()?;
    let arr = v.as_array()?;
    let value = arr.first()?.as_str()?.to_string();
    let start = arr.get(1)?.as_u64()? as usize;
    let end = arr.get(2)?.as_u64()? as usize;
    Some((value, start, end))
}
