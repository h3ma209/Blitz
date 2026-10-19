//! IDE-style comment toggles for multiline editors (⌘/ line, ⌘⇧/ block).

use dioxus::prelude::*;

/// Toggle `// ` line comments for the line range covering `[start, end)`.
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
            if lines.len() == 1 {
                out_lines.push("// ".to_string());
            } else {
                out_lines.push(String::new());
            }
        } else if line.trim().is_empty() {
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

/// True when the event is ⌘/ (toggle line comments).
pub fn is_toggle_line_comment_shortcut(evt: &Event<KeyboardData>) -> bool {
    meta_or_ctrl(evt) && !evt.modifiers().shift() && evt.code() == Code::Slash
}

/// True when the event is ⌘⇧/ (toggle block comments).
pub fn is_toggle_block_comment_shortcut(evt: &Event<KeyboardData>) -> bool {
    meta_or_ctrl(evt) && evt.modifiers().shift() && evt.code() == Code::Slash
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
    fn wrap_selection_in_block_comment() {
        let (out, s, e) = toggle_block_comments("hello", 0, 5);
        assert_eq!(out, "/* hello */");
        assert_eq!((s, e), (0, out.len()));
    }
}
