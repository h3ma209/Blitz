//! Multiline code fields: syntax mirror, `{{` variable menu, Blitz `blitz.*` completions.

use dioxus::prelude::*;

use crate::core::{apply_var_insert, var_insert_context, VariableContext, VariableSuggestion};

use super::comment_toggle::{
    is_toggle_block_comment_shortcut, is_toggle_line_comment_shortcut, toggle_block_comments,
    toggle_line_comments,
};
use super::js_highlight::{render_js_var_mirror_html, render_var_mirror_html};
use super::var_autocomplete::{
    apply_var_completion_dom, read_field_caret_anchor, restore_field_caret, VarSuggestDropdown,
    VarSuggestMenu,
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

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Single snapshot in editor undo/redo history.
#[derive(Clone, Debug, PartialEq)]
pub struct TextSnapshot {
    pub text: String,
    pub caret: usize,
}

/// Robust undo/redo history manager for multiline code editors.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TextHistory {
    pub past: Vec<TextSnapshot>,
    pub future: Vec<TextSnapshot>,
    pub last_checkpoint_ms: u64,
}

impl TextHistory {
    pub fn new(_initial_text: &str) -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            last_checkpoint_ms: current_time_ms(),
        }
    }

    /// Record a keystroke edit, grouping rapid single-character typing.
    pub fn record(&mut self, prev_text: &str, prev_caret: usize, new_text: &str, now_ms: u64) {
        if prev_text == new_text {
            return;
        }
        self.future.clear();

        let is_rapid = now_ms.saturating_sub(self.last_checkpoint_ms) < 450;
        let is_boundary = new_text.ends_with('\n')
            || new_text.ends_with(' ')
            || new_text.ends_with('\t')
            || (new_text.len() as isize - prev_text.len() as isize).abs() > 1;

        if !is_rapid || is_boundary || self.past.is_empty() {
            if self.past.len() >= 150 {
                self.past.remove(0);
            }
            self.past.push(TextSnapshot {
                text: prev_text.to_string(),
                caret: prev_caret,
            });
            self.last_checkpoint_ms = now_ms;
        }
    }

    /// Explicit checkpoint before non-keystroke mutations (e.g. Beautify JSON, comment toggle, indentation).
    pub fn push_explicit_checkpoint(&mut self, text: &str, caret: usize) {
        if self.past.last().map(|s| s.text.as_str()) == Some(text) {
            return;
        }
        self.future.clear();
        if self.past.len() >= 150 {
            self.past.remove(0);
        }
        self.past.push(TextSnapshot {
            text: text.to_string(),
            caret,
        });
        self.last_checkpoint_ms = current_time_ms();
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn undo(&mut self, current_text: &str, current_caret: usize) -> Option<TextSnapshot> {
        let prev = self.past.pop()?;
        if self.future.len() >= 150 {
            self.future.remove(0);
        }
        self.future.push(TextSnapshot {
            text: current_text.to_string(),
            caret: current_caret,
        });
        Some(prev)
    }

    pub fn redo(&mut self, current_text: &str, current_caret: usize) -> Option<TextSnapshot> {
        let next = self.future.pop()?;
        if self.past.len() >= 150 {
            self.past.remove(0);
        }
        self.past.push(TextSnapshot {
            text: current_text.to_string(),
            caret: current_caret,
        });
        Some(next)
    }
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
    #[props(default)] on_submit: Option<EventHandler<()>>,
) -> Element {
    let mut current_val = use_signal(|| value.clone());
    let mut last_sent = use_signal(|| value.clone());
    let mut history = use_signal(|| TextHistory::new(&value));

    if value != last_sent() {
        let prev = current_val();
        if prev != value {
            let len = prev.len();
            history.write().push_explicit_checkpoint(&prev, len);
        }
        current_val.set(value.clone());
        last_sent.set(value.clone());
    }

    let value_for_fallback = current_val();
    let rows_attr = rows.unwrap_or("");
    let field_id = use_signal(|| format!("np-code-{}", uuid::Uuid::new_v4()));
    let mut var_menu = use_signal(|| None::<VarSuggestMenu>);
    let mut js_menu = use_signal(|| None::<JsSuggestMenu>);

    let highlight =
        vars.is_some() || syntax == EditorSyntax::JavaScript || syntax == EditorSyntax::Json;
    let mirror_html = match (syntax, vars.as_ref()) {
        (EditorSyntax::JavaScript, v) => render_js_var_mirror_html(&current_val(), v),
        (EditorSyntax::Json, v) => super::json_highlight::render_json_var_mirror_html(&current_val(), v),
        (_, Some(v)) => render_var_mirror_html(&current_val(), v),
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
            value: "{current_val()}",
            placeholder: "{placeholder}",
            rows: "{rows_attr}",
            spellcheck: "false",
            autocomplete: "off",
            autocorrect: "off",
            autocapitalize: "off",
            oninput: {
                let vars = vars.clone();
                move |evt: Event<FormData>| {
                    let raw_val = evt.value();
                    let new_val = crate::core::sandbox::normalize_smart_quotes(&raw_val);
                    let prev_val = current_val();
                    let now_ms = current_time_ms();
                    let prev_caret = prev_val.len();
                    history.write().record(&prev_val, prev_caret, &new_val, now_ms);

                    current_val.set(new_val.clone());
                    last_sent.set(new_val.clone());
                    on_change.call(new_val.clone());
                    let vars = vars.clone();
                    let field_id = field_id();
                    spawn(async move {
                        sync_code_mirror_scroll(&field_id).await;
                        refresh_editor_menus(
                            &new_val,
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
                move |evt: Event<KeyboardData>| {
                    match evt.key() {
                        Key::Escape => {
                            var_menu.set(None);
                            js_menu.set(None);
                        }
                        _ => {}
                    }
                }
            },
            onclick: {
                let vars = vars.clone();
                move |_| {
                    let vars = vars.clone();
                    let field_id = field_id();
                    spawn(async move {
                        if let Some((caret, left, top, val)) = read_textarea_caret_anchor(&field_id).await {
                            if let Some(v) = vars.as_ref() {
                                if var_insert_context(&val, caret).is_some() {
                                    var_menu.set(VarSuggestMenu::refresh(&val, caret, v, left, top));
                                    js_menu.set(None);
                                    return;
                                }
                            }
                            var_menu.set(None);
                        }
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
                                let value = current_val();
                                spawn(async move {
                                    let caret = read_textarea_caret(&id).await;
                                    history.write().push_explicit_checkpoint(&value, caret);
                                    let res = apply_var_completion_dom(&id, open, &item.insert).await;
                                    if res.is_none() {
                                        if let Some((caret, _, _, _)) = read_textarea_caret_anchor(&id).await {
                                            let (new_text, new_caret) =
                                                apply_var_insert(&value, caret, open, &item.insert);
                                            on_change.call(new_text);
                                            restore_field_caret(&id, new_caret).await;
                                        }
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
                                spawn(async move {
                                    let caret = read_textarea_caret(&id).await;
                                    history.write().push_explicit_checkpoint(&value, caret);
                                    if let Some((caret, _, _, _)) = read_textarea_caret_anchor(&id).await {
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

                // Undo / Redo keyboard shortcuts
                let cmd = evt.modifiers().meta() || evt.modifiers().ctrl();
                let shift = evt.modifiers().shift();
                let is_undo = cmd && !shift && matches!(evt.key(), Key::Character(ref c) if c.eq_ignore_ascii_case("z"));
                let is_redo = (cmd && shift && matches!(evt.key(), Key::Character(ref c) if c.eq_ignore_ascii_case("z")))
                    || (cmd && !shift && matches!(evt.key(), Key::Character(ref c) if c.eq_ignore_ascii_case("y")));

                if is_undo {
                    evt.prevent_default();
                    let id = field_id();
                    let cur = current_val();
                    spawn(async move {
                        let caret = read_textarea_caret(&id).await;
                        if let Some(snap) = history.write().undo(&cur, caret) {
                            current_val.set(snap.text.clone());
                            last_sent.set(snap.text.clone());
                            on_change.call(snap.text.clone());
                            apply_editor_undo_redo(&id, &snap.text, snap.caret).await;
                        }
                    });
                    return;
                }

                if is_redo {
                    evt.prevent_default();
                    let id = field_id();
                    let cur = current_val();
                    spawn(async move {
                        let caret = read_textarea_caret(&id).await;
                        if let Some(snap) = history.write().redo(&cur, caret) {
                            current_val.set(snap.text.clone());
                            last_sent.set(snap.text.clone());
                            on_change.call(snap.text.clone());
                            apply_editor_undo_redo(&id, &snap.text, snap.caret).await;
                        }
                    });
                    return;
                }

                if cmd && evt.key() == Key::Enter {
                    if let Some(ref h) = on_submit {
                        evt.prevent_default();
                        h.call(());
                        return;
                    }
                }

                // Auto-indent on Enter (VS Code style block matching and indent preservation)
                if evt.key() == Key::Enter
                    && !evt.modifiers().shift()
                    && !evt.modifiers().meta()
                    && !evt.modifiers().ctrl()
                    && !evt.modifiers().alt()
                {
                    evt.prevent_default();
                    let id = field_id();
                    let cur = current_val();
                    spawn(async move {
                        let caret = read_textarea_caret(&id).await;
                        history.write().push_explicit_checkpoint(&cur, caret);
                        auto_indent_on_enter(&id).await;
                    });
                    return;
                }

                // Instant DOM-direct Tab indentation / Shift+Tab dedentation
                if evt.key() == Key::Tab {
                    evt.prevent_default();
                    let shift = evt.modifiers().shift();
                    let id = field_id();
                    let cur = current_val();
                    spawn(async move {
                        let caret = read_textarea_caret(&id).await;
                        history.write().push_explicit_checkpoint(&cur, caret);
                        indent_tab_dom(&id, shift).await;
                    });
                    return;
                }

                let line = is_toggle_line_comment_shortcut(&evt);
                let block = is_toggle_block_comment_shortcut(&evt);
                if !line && !block {
                    return;
                }
                evt.prevent_default();
                let fallback = value_for_fallback.clone();
                let id = field_id();
                let cur = current_val();
                spawn(async move {
                    let caret = read_textarea_caret(&id).await;
                    history.write().push_explicit_checkpoint(&cur, caret);
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
            div { class: "code-editor-quick-actions",
                button {
                    class: "code-action-btn",
                    r#type: "button",
                    title: "Undo (⌘Z)",
                    disabled: !history().can_undo(),
                    onclick: {
                        let id = field_id();
                        let cur = current_val();
                        move |_| {
                            let id = id.clone();
                            let cur = cur.clone();
                            spawn(async move {
                                let caret = read_textarea_caret(&id).await;
                                if let Some(snap) = history.write().undo(&cur, caret) {
                                    current_val.set(snap.text.clone());
                                    last_sent.set(snap.text.clone());
                                    on_change.call(snap.text.clone());
                                    apply_editor_undo_redo(&id, &snap.text, snap.caret).await;
                                }
                            });
                        }
                    },
                    span { class: "action-icon action-icon-undo", aria_hidden: "true" }
                }
                button {
                    class: "code-action-btn",
                    r#type: "button",
                    title: "Redo (⌘⇧Z)",
                    disabled: !history().can_redo(),
                    onclick: {
                        let id = field_id();
                        let cur = current_val();
                        move |_| {
                            let id = id.clone();
                            let cur = cur.clone();
                            spawn(async move {
                                let caret = read_textarea_caret(&id).await;
                                if let Some(snap) = history.write().redo(&cur, caret) {
                                    current_val.set(snap.text.clone());
                                    last_sent.set(snap.text.clone());
                                    on_change.call(snap.text.clone());
                                    apply_editor_undo_redo(&id, &snap.text, snap.caret).await;
                                }
                            });
                        }
                    },
                    span { class: "action-icon action-icon-redo", aria_hidden: "true" }
                }
            }
            if let Some(menu) = var_menu() {
                if vars.is_some() {
                    VarSuggestDropdown {
                        menu: menu.clone(),
                        on_pick: {
                            let value = current_val();
                            move |item: VariableSuggestion| {
                                let open = var_menu().map(|m| m.open_at).unwrap_or(0);
                                let id = field_id();
                                let value = value.clone();
                                spawn(async move {
                                    let res = apply_var_completion_dom(&id, open, &item.insert).await;
                                    if res.is_none() {
                                        if let Some((caret, _, _, _)) = read_textarea_caret_anchor(&id).await {
                                            let (new_text, new_caret) =
                                                apply_var_insert(&value, caret, open, &item.insert);
                                            on_change.call(new_text);
                                            restore_field_caret(&id, new_caret).await;
                                        }
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
                                    let value = value.clone();
                                    move |_| {
                                        let id = field_id();
                                        let value = value.clone();
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

async fn auto_indent_on_enter(textarea_id: &str) -> bool {
    let script = format!(
        r#"
        const el = document.getElementById({id});
        if (!el) return false;
        const val = el.value || '';
        const start = el.selectionStart ?? 0;
        const end = el.selectionEnd ?? 0;
        const before = val.slice(0, start);
        const after = val.slice(end);

        const lastNewline = before.lastIndexOf('\n');
        const lineStart = lastNewline === -1 ? 0 : lastNewline + 1;
        const currentLine = before.slice(lineStart);

        const indentMatch = currentLine.match(/^[ \t]*/);
        const baseIndent = indentMatch ? indentMatch[0] : '';

        const trimmedBefore = currentLine.trimEnd();
        const opensBlock = trimmedBefore.endsWith('{{') || trimmedBefore.endsWith('[');

        const afterTrim = after.trimStart();
        const nextChar = afterTrim.length > 0 ? afterTrim[0] : '';
        const closesBlock = (trimmedBefore.endsWith('{{') && nextChar === '}}') ||
                            (trimmedBefore.endsWith('[') && nextChar === ']');

        let insert = '';
        let caretOffset = 0;
        if (opensBlock && closesBlock) {{
            insert = '\n' + baseIndent + '  \n' + baseIndent;
            caretOffset = 1 + baseIndent.length + 2;
        }} else if (opensBlock) {{
            insert = '\n' + baseIndent + '  ';
            caretOffset = insert.length;
        }} else {{
            insert = '\n' + baseIndent;
            caretOffset = insert.length;
        }}

        el.value = before + insert + after;
        const newCaret = start + caretOffset;
        el.setSelectionRange(newCaret, newCaret);
        const host = el.closest('.code-editor-host, .var-highlight-host');
        const mirror = host && host.querySelector('.code-editor-mirror, .var-highlight-mirror');
        if (mirror) {{
            mirror.scrollLeft = el.scrollLeft || 0;
            mirror.scrollTop = el.scrollTop || 0;
        }}
        el.dispatchEvent(new Event('input', {{ bubbles: true }}));
        return true;
        "#,
        id = serde_json::to_string(textarea_id).unwrap_or_else(|_| "\"\"".into())
    );
    document::eval(&script).await.ok().and_then(|v| v.as_bool()).unwrap_or(false)
}

async fn indent_tab_dom(textarea_id: &str, shift: bool) -> bool {
    let script = format!(
        r#"
        const el = document.getElementById({id});
        if (!el) return false;
        const val = el.value || '';
        const start = el.selectionStart ?? 0;
        const end = el.selectionEnd ?? 0;
        const shift = {shift};

        if (start === end && !shift) {{
            const before = val.slice(0, start);
            const after = val.slice(end);
            el.value = before + '  ' + after;
            const newCaret = start + 2;
            el.setSelectionRange(newCaret, newCaret);
        }} else {{
            const selStart = Math.min(start, end);
            const selEnd = Math.max(start, end);
            const lastNl = val.lastIndexOf('\n', selStart - 1);
            const lineStart = lastNl === -1 ? 0 : lastNl + 1;
            const nextNl = val.indexOf('\n', selEnd);
            const lineEnd = nextNl === -1 ? val.length : nextNl;

            const block = val.slice(lineStart, lineEnd);
            const lines = block.split('\n');
            let outLines = [];
            if (shift) {{
                for (const l of lines) {{
                    if (l.startsWith('  ')) outLines.push(l.slice(2));
                    else if (l.startsWith(' ')) outLines.push(l.slice(1));
                    else if (l.startsWith('\t')) outLines.push(l.slice(1));
                    else outLines.push(l);
                }}
            }} else {{
                for (const l of lines) {{
                    outLines.push('  ' + l);
                }}
            }}
            const joined = outLines.join('\n');
            el.value = val.slice(0, lineStart) + joined + val.slice(lineEnd);
            const newEnd = lineStart + joined.length;
            el.setSelectionRange(lineStart, newEnd);
        }}
        const host = el.closest('.code-editor-host, .var-highlight-host');
        const mirror = host && host.querySelector('.code-editor-mirror, .var-highlight-mirror');
        if (mirror) {{
            mirror.scrollLeft = el.scrollLeft || 0;
            mirror.scrollTop = el.scrollTop || 0;
        }}
        el.dispatchEvent(new Event('input', {{ bubbles: true }}));
        return true;
        "#,
        id = serde_json::to_string(textarea_id).unwrap_or_else(|_| "\"\"".into()),
        shift = shift
    );
    document::eval(&script).await.ok().and_then(|v| v.as_bool()).unwrap_or(false)
}

async fn read_textarea_caret_anchor(element_id: &str) -> Option<(usize, f64, f64, String)> {
    let script = format!(
        r#"
        const el = document.getElementById({id});
        if (!el) return null;
        const rect = el.getBoundingClientRect();
        const host = el.closest('.var-hover-host, .code-editor-host');
        const hr = host ? host.getBoundingClientRect() : rect;
        const val = el.value || '';
        const caretU16 = el.selectionStart ?? 0;
        const sub = val.slice(0, caretU16);
        const byteLen = new TextEncoder().encode(sub).length;

        // Calculate line number and column position for caret
        const lines = sub.split('\n');
        const lineIdx = lines.length - 1;
        const curLineText = lines[lineIdx];

        // Read computed styling
        const style = getComputedStyle(el);
        const padTop = parseFloat(style.paddingTop) || 8;
        const padLeft = parseFloat(style.paddingLeft) || 12;
        const fontSize = parseFloat(style.fontSize) || 14;
        const lineHeight = parseFloat(style.lineHeight) || (fontSize * 1.5);

        // Approximate character width from font
        const canvas = document.createElement('canvas');
        const ctx = canvas.getContext('2d');
        ctx.font = (style.fontStyle || 'normal') + ' ' + (style.fontWeight || '400') + ' ' + fontSize + 'px ' + (style.fontFamily || 'monospace');
        const textWidth = ctx.measureText(curLineText).width;

        // Top position directly below the current line
        const lineTop = padTop + (lineIdx * lineHeight) - (el.scrollTop || 0);
        const lineBottom = lineTop + lineHeight + 2;

        const leftPos = Math.max(8, padLeft + textWidth - (el.scrollLeft || 0));
        // Keep within reasonable host bounds
        const maxLeft = Math.max(16, (hr.width || 400) - 240);
        const clampedLeft = Math.min(leftPos, maxLeft);

        return [byteLen, clampedLeft, lineBottom, val];
        "#,
        id = serde_json::to_string(element_id).unwrap_or_else(|_| "\"\"".into())
    );
    let v = document::eval(&script).await.ok()?;
    let arr = v.as_array()?;
    let idx = arr.first()?.as_u64()? as usize;
    let left = arr.get(1)?.as_f64().or_else(|| arr.get(1)?.as_u64().map(|n| n as f64))?;
    let top = arr.get(2)?.as_f64().or_else(|| arr.get(2)?.as_u64().map(|n| n as f64))?;
    let val = arr.get(3)?.as_str().unwrap_or("").to_string();
    Some((idx, left, top, val))
}

async fn refresh_editor_menus(
    _fallback_value: &str,
    vars: Option<&VariableContext>,
    js_completions: bool,
    field_id: &str,
    mut var_menu: Signal<Option<VarSuggestMenu>>,
    mut js_menu: Signal<Option<JsSuggestMenu>>,
) {
    if let Some((caret, left, top, value)) = read_textarea_caret_anchor(field_id).await {
        if let Some(v) = vars {
            if var_insert_context(&value, caret).is_some() {
                var_menu.set(VarSuggestMenu::refresh(&value, caret, v, left, top));
                js_menu.set(None);
                return;
            }
        }
        var_menu.set(None);
        if js_completions {
            if let Some((start, prefix)) = js_complete_prefix(&value, caret) {
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

async fn read_textarea_caret(id: &str) -> usize {
    let script = format!(
        r#"
        const el = document.getElementById({id});
        if (!el) return 0;
        const val = el.value || '';
        const startU16 = el.selectionStart ?? val.length;
        return new TextEncoder().encode(val.slice(0, startU16)).length;
        "#,
        id = serde_json::to_string(id).unwrap_or_else(|_| "\"\"".into())
    );
    document::eval(&script).await.ok().and_then(|v| v.as_u64()).unwrap_or(0) as usize
}

async fn apply_editor_undo_redo(textarea_id: &str, text: &str, byte_caret: usize) {
    let script = format!(
        r#"
        const el = document.getElementById({id});
        if (el) {{
            el.value = {text};
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
            el.focus();
            try {{
                el.setSelectionRange(u16Pos, u16Pos);
            }} catch (_) {{}}
            const host = el.closest('.code-editor-host, .var-highlight-host');
            const mirror = host && host.querySelector('.code-editor-mirror, .var-highlight-mirror');
            if (mirror) {{
                mirror.scrollLeft = el.scrollLeft || 0;
                mirror.scrollTop = el.scrollTop || 0;
            }}
        }}
        "#,
        id = serde_json::to_string(textarea_id).unwrap_or_else(|_| "\"\"".into()),
        text = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into()),
        byte_caret = byte_caret
    );
    let _ = document::eval(&script).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_history_undo_redo_cycle() {
        let mut h = TextHistory::new("");
        assert!(!h.can_undo());
        assert!(!h.can_redo());

        h.record("", 0, "hello", 1000);
        assert!(h.can_undo());
        assert!(!h.can_redo());

        h.record("hello", 5, "hello world", 1600);
        assert_eq!(h.past.len(), 2);

        let snap1 = h.undo("hello world", 11).expect("undo 1");
        assert_eq!(snap1.text, "hello");
        assert!(h.can_redo());

        let snap0 = h.undo("hello", 5).expect("undo 2");
        assert_eq!(snap0.text, "");
        assert!(!h.can_undo());

        let redo1 = h.redo("", 0).expect("redo 1");
        assert_eq!(redo1.text, "hello");

        let redo2 = h.redo("hello", 5).expect("redo 2");
        assert_eq!(redo2.text, "hello world");
        assert!(!h.can_redo());
    }

    #[test]
    fn test_text_history_explicit_checkpoint() {
        let mut h = TextHistory::new("raw json");
        h.push_explicit_checkpoint("raw json", 8);
        assert!(h.can_undo());

        let snap = h.undo("pretty json", 12).expect("undo");
        assert_eq!(snap.text, "raw json");
        assert_eq!(snap.caret, 8);

        let redo = h.redo("raw json", 8).expect("redo");
        assert_eq!(redo.text, "pretty json");
    }

    #[test]
    fn test_text_history_rapid_typing_grouping() {
        let mut h = TextHistory::new("");
        // Fast keystrokes within 200ms
        h.record("", 0, "a", 1000);
        h.record("a", 1, "ab", 1100);
        h.record("ab", 2, "abc", 1200);
        // Only 1 checkpoint should be created for the initial transition
        assert_eq!(h.past.len(), 1);

        // Later keystroke after 600ms
        h.record("abc", 3, "abcd", 1900);
        assert_eq!(h.past.len(), 2);
    }
}
