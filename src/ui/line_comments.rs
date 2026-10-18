//! IDE-style comment toggles for multiline editors (⌘/ line, ⌘⇧/ block).

use dioxus::prelude::*;

use crate::core::{render_var_highlight_html, VariableContext};

/// Toggle `// ` line comments for the line range covering `[start, end)`.
///
/// - Selection (or caret line if `start == end`) expands to whole lines.
/// - If every non-empty line is already commented → uncomment; else comment.
/// - Comment marker is inserted after leading whitespace: `// `.
/// - Returns `(new_text, new_start, new_end)` covering the edited block.
pub fn toggle_line_comments(text: &str, start: usize, end: usize) -> (String, usize, usize) {
    let len = text.len();
    let start = start.min(len);
    let end = end.min(len);
    let (sel_start, sel_end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };

    let (block_start, block_end) = line_block_bounds(text, sel_start, sel_end);
    let block = &text[block_start..block_end];
    let lines: Vec<&str> = split_lines_keep_empty(block);

    let should_uncomment = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .all(|l| is_line_commented(l))
        && lines.iter().any(|l| !l.trim().is_empty());

    let mut out_lines: Vec<String> = Vec::with_capacity(lines.len());
    for line in &lines {
        if should_uncomment {
            out_lines.push(uncomment_line(line));
        } else if line.is_empty() {
            // Empty caret line → `// `; blank lines inside a multi-line block stay blank.
            if lines.len() == 1 {
                out_lines.push("// ".to_string());
            } else {
                out_lines.push(String::new());
            }
        } else if line.trim().is_empty() {
            // Whitespace-only: leave unchanged when commenting a block.
            out_lines.push((*line).to_string());
        } else {
            out_lines.push(comment_line(line));
        }
    }

    let joined = out_lines.join("\n");
    let new_block = if !block.is_empty() && block.ends_with('\n') {
        format!("{joined}\n")
    } else {
        joined
    };

    let mut new_text = String::with_capacity(text.len() + out_lines.len() * 3);
    new_text.push_str(&text[..block_start]);
    new_text.push_str(&new_block);
    new_text.push_str(&text[block_end..]);

    let new_start = block_start;
    let new_end = block_start + new_block.len();
    (new_text, new_start, new_end)
}

fn line_block_bounds(text: &str, start: usize, end: usize) -> (usize, usize) {
    let mut end = end;
    // If selection ends at column 0 of a line (right after `\n`), exclude that next line.
    if end > start && text.as_bytes().get(end.wrapping_sub(1)) == Some(&b'\n') {
        end -= 1;
    }
    let block_start = text[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let block_end = match text[end..].find('\n') {
        Some(i) => end + i,
        None => text.len(),
    };
    (block_start, block_end)
}

fn split_lines_keep_empty(block: &str) -> Vec<&str> {
    if block.is_empty() {
        return vec![""];
    }
    // Do not keep a trailing empty segment solely from a final `\n`.
    let trim_end = block.ends_with('\n');
    let slice = if trim_end {
        &block[..block.len() - 1]
    } else {
        block
    };
    if slice.is_empty() {
        return vec![""];
    }
    slice.split('\n').collect()
}

fn is_line_commented(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

fn comment_line(line: &str) -> String {
    let ws = leading_whitespace(line);
    let rest = &line[ws.len()..];
    format!("{ws}// {rest}")
}

fn uncomment_line(line: &str) -> String {
    let ws = leading_whitespace(line);
    let rest = &line[ws.len()..];
    if let Some(after) = rest.strip_prefix("// ") {
        format!("{ws}{after}")
    } else if let Some(after) = rest.strip_prefix("//") {
        format!("{ws}{after}")
    } else {
        line.to_string()
    }
}

fn leading_whitespace(line: &str) -> &str {
    let trimmed = line.trim_start();
    &line[..line.len() - trimmed.len()]
}

/// Toggle `/* … */` around `[start, end)`.
///
/// - If selection (trimmed) is already wrapped in `/*` / `*/` → unwrap.
/// - Else wrap selection; empty caret inserts `/*  */` with inner range selected.
/// - Returns `(new_text, new_start, new_end)`.
pub fn toggle_block_comments(text: &str, start: usize, end: usize) -> (String, usize, usize) {
    let len = text.len();
    let start = start.min(len);
    let end = end.min(len);
    let (sel_start, sel_end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let selected = &text[sel_start..sel_end];
    let trimmed = selected.trim();
    let lead = selected.len() - selected.trim_start().len();
    let trail = selected.len() - selected.trim_end().len();

    if let Some(inner) = strip_block_comment(trimmed) {
        let inner_start = sel_start + lead;
        let inner_end = sel_end - trail;
        let mut new_text = String::with_capacity(text.len());
        new_text.push_str(&text[..inner_start]);
        new_text.push_str(inner);
        new_text.push_str(&text[inner_end..]);
        let new_start = sel_start + lead;
        let new_end = new_start + inner.len();
        return (new_text, new_start, new_end);
    }

    let open = "/* ";
    let close = " */";
    let mut new_text = String::with_capacity(text.len() + open.len() + close.len());
    new_text.push_str(&text[..sel_start]);
    new_text.push_str(open);
    new_text.push_str(selected);
    new_text.push_str(close);
    new_text.push_str(&text[sel_end..]);
    let new_start = sel_start;
    let new_end = sel_start + open.len() + selected.len() + close.len();
    (new_text, new_start, new_end)
}

fn strip_block_comment(trimmed: &str) -> Option<&str> {
    let after_open = trimmed.strip_prefix("/*")?;
    let before_close = after_open.strip_suffix("*/")?;
    let inner = before_close.strip_prefix(' ').unwrap_or(before_close);
    let inner = inner.strip_suffix(' ').unwrap_or(inner);
    Some(inner)
}

fn meta_or_ctrl(evt: &Event<KeyboardData>) -> bool {
    evt.modifiers().meta() || evt.modifiers().ctrl()
}

pub fn is_toggle_line_comment_shortcut(evt: &Event<KeyboardData>) -> bool {
    meta_or_ctrl(evt) && !evt.modifiers().shift() && evt.code() == Code::Slash
}

pub fn is_toggle_block_comment_shortcut(evt: &Event<KeyboardData>) -> bool {
    meta_or_ctrl(evt) && evt.modifiers().shift() && evt.code() == Code::Slash
}

/// Shared multiline editor with ⌘/ line + ⌘⇧/ block comment toggles.
/// When `vars` is set, `{{…}}` tokens get a highlight mirror under transparent text.
#[component]
pub fn CommentableTextarea(
    class: String,
    value: String,
    on_change: EventHandler<String>,
    #[props(default)] placeholder: String,
    #[props(default)] rows: Option<&'static str>,
    #[props(default)] vars: Option<VariableContext>,
) -> Element {
    let value_for_fallback = value.clone();
    let rows_attr = rows.unwrap_or("");
    let ta_id = use_signal(|| format!("np-ta-{}", uuid::Uuid::new_v4()));
    let highlight = vars.is_some();
    let mirror_html = vars
        .as_ref()
        .map(|v| render_var_highlight_html(&value, Some(v)))
        .unwrap_or_default();
    let ta_class = if highlight {
        format!("{class} var-highlight-input")
    } else {
        class.clone()
    };

    let textarea = rsx! {
        textarea {
            id: "{ta_id()}",
            class: "{ta_class}",
            value: "{value}",
            placeholder: "{placeholder}",
            rows: "{rows_attr}",
            oninput: move |evt: Event<FormData>| on_change.call(evt.value()),
            onscroll: move |_| {
                if !highlight {
                    return;
                }
                let id = ta_id();
                spawn(async move {
                    sync_textarea_mirror_scroll(&id).await;
                });
            },
            onkeydown: move |evt: Event<KeyboardData>| {
                let line = is_toggle_line_comment_shortcut(&evt);
                let block = is_toggle_block_comment_shortcut(&evt);
                if !line && !block {
                    return;
                }
                evt.prevent_default();
                let fallback = value_for_fallback.clone();
                let on_change = on_change.clone();
                spawn(async move {
                    let (text, start, end) = match read_active_textarea_state().await {
                        Some(state) => state,
                        None => {
                            let len = fallback.len();
                            (fallback, len, len)
                        }
                    };
                    let (new_text, new_start, new_end) = if block {
                        toggle_block_comments(&text, start, end)
                    } else {
                        toggle_line_comments(&text, start, end)
                    };
                    if new_text != text {
                        on_change.call(new_text);
                    }
                    restore_active_textarea_selection(new_start, new_end).await;
                });
            },
        }
    };

    if highlight {
        rsx! {
            div {
                class: "var-highlight-host var-highlight-host-multiline",
                div {
                    class: "var-highlight-mirror var-highlight-mirror-multiline",
                    "aria-hidden": "true",
                    dangerous_inner_html: "{mirror_html}",
                }
                {textarea}
            }
        }
    } else {
        textarea
    }
}

async fn sync_textarea_mirror_scroll(textarea_id: &str) {
    let script = format!(
        r#"
        const el = document.getElementById({id});
        if (!el) return false;
        const host = el.closest('.var-highlight-host');
        const mirror = host && host.querySelector('.var-highlight-mirror');
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

async fn read_active_textarea_state() -> Option<(String, usize, usize)> {
    let eval = document::eval(
        r#"
        const el = document.activeElement;
        if (!el || el.tagName !== 'TEXTAREA') return null;
        return [el.value, el.selectionStart ?? 0, el.selectionEnd ?? 0];
        "#,
    );
    let v = eval.await.ok()?;
    let arr = v.as_array()?;
    let value = arr.first()?.as_str()?.to_string();
    let start = arr.get(1)?.as_u64()? as usize;
    let end = arr.get(2)?.as_u64()? as usize;
    Some((value, start, end))
}

async fn restore_active_textarea_selection(start: usize, end: usize) {
    let script = format!(
        r#"
        await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
        const el = document.activeElement;
        if (el && el.tagName === 'TEXTAREA') {{
            el.focus();
            try {{ el.setSelectionRange({start}, {end}); }} catch (_) {{}}
        }}
        return true;
        "#
    );
    let _ = document::eval(&script).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_single_line_at_caret() {
        let (out, s, e) = toggle_line_comments("hello", 2, 2);
        assert_eq!(out, "// hello");
        assert_eq!((s, e), (0, out.len()));
    }

    #[test]
    fn uncomment_single_line() {
        let (out, _, _) = toggle_line_comments("// hello", 0, 0);
        assert_eq!(out, "hello");
    }

    #[test]
    fn uncomment_without_space_after_slashes() {
        let (out, _, _) = toggle_line_comments("//hello", 0, 3);
        assert_eq!(out, "hello");
    }

    #[test]
    fn preserves_indent_when_commenting() {
        let (out, _, _) = toggle_line_comments("    foo", 4, 4);
        assert_eq!(out, "    // foo");
    }

    #[test]
    fn preserves_indent_when_uncommenting() {
        let (out, _, _) = toggle_line_comments("    // foo", 0, 0);
        assert_eq!(out, "    foo");
    }

    #[test]
    fn comment_selection_multiple_lines() {
        let text = "a\nb\nc";
        let (out, _, _) = toggle_line_comments(text, 0, text.len());
        assert_eq!(out, "// a\n// b\n// c");
    }

    #[test]
    fn uncomment_selection_multiple_lines() {
        let text = "// a\n// b\n// c";
        let (out, _, _) = toggle_line_comments(text, 0, text.len());
        assert_eq!(out, "a\nb\nc");
    }

    #[test]
    fn mixed_selection_comments_all() {
        let text = "// a\nb\n// c";
        let (out, _, _) = toggle_line_comments(text, 0, text.len());
        assert_eq!(out, "// // a\n// b\n// // c");
    }

    #[test]
    fn only_touches_selected_lines() {
        let text = "keep\ntarget\nkeep";
        let start = text.find('t').unwrap();
        let (out, _, _) = toggle_line_comments(text, start, start + 3);
        assert_eq!(out, "keep\n// target\nkeep");
    }

    #[test]
    fn selection_ending_at_next_line_start_excludes_it() {
        let text = "one\ntwo\nthree";
        let end = text.find('t').unwrap();
        let (out, _, _) = toggle_line_comments(text, 0, end);
        assert_eq!(out, "// one\ntwo\nthree");
    }

    #[test]
    fn empty_line_caret_inserts_comment_marker() {
        let (out, _, _) = toggle_line_comments("", 0, 0);
        assert_eq!(out, "// ");
    }

    #[test]
    fn middle_empty_lines_preserved_when_commenting_block() {
        let text = "a\n\nb";
        let (out, _, _) = toggle_line_comments(text, 0, text.len());
        assert_eq!(out, "// a\n\n// b");
    }

    #[test]
    fn wrap_selection_in_block_comment() {
        let (out, s, e) = toggle_block_comments("hello", 0, 5);
        assert_eq!(out, "/* hello */");
        assert_eq!((s, e), (0, out.len()));
    }

    #[test]
    fn unwrap_block_comment_selection() {
        let (out, _, _) = toggle_block_comments("/* hello */", 0, 11);
        assert_eq!(out, "hello");
    }

    #[test]
    fn unwrap_block_without_inner_spaces() {
        let (out, _, _) = toggle_block_comments("/*hello*/", 0, 9);
        assert_eq!(out, "hello");
    }

    #[test]
    fn empty_caret_inserts_block_markers() {
        let (out, s, e) = toggle_block_comments("ab", 1, 1);
        assert_eq!(out, "a/*  */b");
        assert_eq!((s, e), (1, 7));
    }

    #[test]
    fn block_wrap_only_touches_selection() {
        let text = "keep target keep";
        let start = text.find('t').unwrap();
        let end = start + "target".len();
        let (out, _, _) = toggle_block_comments(text, start, end);
        assert_eq!(out, "keep /* target */ keep");
    }
}
