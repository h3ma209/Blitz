//! Variable hover popover: inspect / edit `{{var}}` under caret or mouse.

use std::time::Duration;

use dioxus::prelude::*;

use crate::core::{
    apply_var_insert, find_var_at, render_var_highlight_html, var_insert_context, VarHoverInfo,
    VarResolveScope, VariableContext, VariableSuggestion,
};

use super::var_autocomplete::{
    read_field_caret_anchor, restore_field_caret, VarSuggestDropdown, VarSuggestMenu,
};
use super::DismissBackdrop;

/// Dwell time: mouse must hover on a variable token before popover appears (prevents rapid cursor flash).
const HOVER_DWELL_MS: u64 = 160;
/// Fast switch dwell when a popover is already visible and pointer moves to another token.
const HOVER_SWITCH_MS: u64 = 50;
/// Grace window so pointer can travel from token → popover without flicker.
const DISMISS_GRACE_MS: u64 = 260;

/// Edit applied from the hover popover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarEditEvent {
    Save {
        name: String,
        raw: String,
        scope: VarResolveScope,
        value: String,
    },
    AddToEnvironment {
        name: String,
        value: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct PopoverState {
    info: VarHoverInfo,
    left: f64,
    top: f64,
}

#[allow(clippy::too_many_arguments)]
fn try_open_popover(
    text: &str,
    vars: &VariableContext,
    index: usize,
    left: f64,
    top: f64,
    mut popover: Signal<Option<PopoverState>>,
    mut draft: Signal<String>,
    mut dismiss_gen: Signal<u64>,
) {
    if let Some(span) = find_var_at(text, index) {
        // Cancel any pending dismiss when (re)opening.
        dismiss_gen.set(dismiss_gen() + 1);
        let info = vars.inspect_var(&span.raw);
        draft.set(info.value.clone());
        popover.set(Some(PopoverState { info, left, top }));
    }
}

/// After grace, close unless value input focused or pointer is over the popover.
fn schedule_dismiss(
    mut popover: Signal<Option<PopoverState>>,
    mut dismiss_gen: Signal<u64>,
    editing: Signal<bool>,
    over_popover: Signal<bool>,
) {
    if editing() {
        return;
    }
    let gen = dismiss_gen() + 1;
    dismiss_gen.set(gen);
    spawn(async move {
        tokio::time::sleep(Duration::from_millis(DISMISS_GRACE_MS)).await;
        if dismiss_gen() == gen && !editing() && !over_popover() {
            popover.set(None);
        }
    });
}

fn cancel_dismiss(mut dismiss_gen: Signal<u64>) {
    dismiss_gen.set(dismiss_gen() + 1);
}

/// Text input that opens a floating editor when caret/hover hits `{{…}}`.
/// Mirror layer under transparent text colors `{{…}}` tokens (Postman-style).
#[component]
pub fn VarHoverInput(
    class: String,
    value: String,
    placeholder: String,
    input_id: String,
    vars: VariableContext,
    on_input: EventHandler<String>,
    on_var_edit: EventHandler<VarEditEvent>,
) -> Element {
    let mut popover = use_signal(|| None::<PopoverState>);
    let mut draft = use_signal(String::new);
    let mut editing = use_signal(|| false);
    let mut over_popover = use_signal(|| false);
    let mut dismiss_gen = use_signal(|| 0u64);
    let mut hover_seq = use_signal(|| 0u64);
    let mut var_suggest = use_signal(|| None::<VarSuggestMenu>);
    let mirror_html = render_var_highlight_html(&value, Some(&vars));
    let input_class = format!("{class} var-highlight-input");

    rsx! {
        div {
            class: "var-hover-host var-highlight-host code-editor-host",
            onmouseleave: move |_| {
                hover_seq.set(hover_seq() + 1);
                schedule_dismiss(popover, dismiss_gen, editing, over_popover);
            },
            div {
                class: "var-highlight-mirror",
                "aria-hidden": "true",
                dangerous_inner_html: "{mirror_html}",
            }
            input {
                id: "{input_id}",
                class: "{input_class}",
                r#type: "text",
                value: "{value}",
                placeholder: "{placeholder}",
                oninput: {
                    let vars = vars.clone();
                    let input_id = input_id.clone();
                    move |evt| {
                        popover.set(None);
                        let val = evt.value();
                        on_input.call(val.clone());
                        let vars = vars.clone();
                        let input_id = input_id.clone();
                        spawn(async move {
                            if let Some((caret, left, top)) = read_field_caret_anchor(&input_id).await {
                                if var_insert_context(&val, caret).is_some() {
                                    var_suggest.set(VarSuggestMenu::refresh(&val, caret, &vars, left, top));
                                } else {
                                    var_suggest.set(None);
                                }
                            }
                        });
                    }
                },
                onkeydown: {
                    let value = value.clone();
                    let input_id = input_id.clone();
                    move |evt: Event<KeyboardData>| {
                        if let Some(mut m) = var_suggest() {
                            match evt.key() {
                                Key::ArrowDown => {
                                    evt.prevent_default();
                                    m.move_selection(1);
                                    var_suggest.set(Some(m));
                                }
                                Key::ArrowUp => {
                                    evt.prevent_default();
                                    m.move_selection(-1);
                                    var_suggest.set(Some(m));
                                }
                                Key::Escape => {
                                    evt.prevent_default();
                                    var_suggest.set(None);
                                }
                                Key::Enter | Key::Tab => {
                                    if let Some(item) = m.pick() {
                                        evt.prevent_default();
                                        let open = m.open_at;
                                        let item = item.clone();
                                        let value = value.clone();
                                        let input_id = input_id.clone();
                                        spawn(async move {
                                            if let Some((caret, _, _)) =
                                                read_field_caret_anchor(&input_id).await
                                            {
                                                let (new_text, new_caret) =
                                                    apply_var_insert(&value, caret, open, &item.insert);
                                                on_input.call(new_text);
                                                restore_field_caret(&input_id, new_caret).await;
                                            }
                                        });
                                        var_suggest.set(None);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                },
                onscroll: {
                    let input_id = input_id.clone();
                    move |_| {
                        let input_id = input_id.clone();
                        spawn(async move {
                            sync_mirror_scroll(&input_id).await;
                        });
                    }
                },
                onclick: {
                    let input_id = input_id.clone();
                    let value = value.clone();
                    let vars = vars.clone();
                    move |_| {
                        let input_id = input_id.clone();
                        let value = value.clone();
                        let vars = vars.clone();
                        spawn(async move {
                            if let Some((idx, left, top)) = read_caret_and_anchor(&input_id).await {
                                try_open_popover(
                                    &value, &vars, idx, left, top, popover, draft, dismiss_gen,
                                );
                            }
                        });
                    }
                },
                onkeyup: {
                    let input_id = input_id.clone();
                    let value = value.clone();
                    let vars = vars.clone();
                    move |evt: Event<KeyboardData>| {
                        let value_s = value.clone();
                        let vars_for_suggest = vars.clone();
                        let input_id_for_suggest = input_id.clone();
                        spawn(async move {
                            if let Some((caret, left, top)) =
                                read_field_caret_anchor(&input_id_for_suggest).await
                            {
                                if var_insert_context(&value_s, caret).is_some() {
                                    var_suggest.set(VarSuggestMenu::refresh(
                                        &value_s,
                                        caret,
                                        &vars_for_suggest,
                                        left,
                                        top,
                                    ));
                                } else {
                                    var_suggest.set(None);
                                }
                            }
                        });
                        if matches!(evt.key(), Key::Escape) {
                            dismiss_gen.set(dismiss_gen() + 1);
                            editing.set(false);
                            popover.set(None);
                            return;
                        }
                        let input_id = input_id.clone();
                        let value = value.clone();
                        let vars = vars.clone();
                        spawn(async move {
                            if let Some((idx, left, top)) = read_caret_and_anchor(&input_id).await {
                                try_open_popover(
                                    &value, &vars, idx, left, top, popover, draft, dismiss_gen,
                                );
                            }
                        });
                    }
                },
                onmousemove: {
                    let input_id = input_id.clone();
                    let value = value.clone();
                    let vars = vars.clone();
                    move |evt: Event<MouseData>| {
                        if editing() {
                            return;
                        }
                        let client_x = evt.client_coordinates().x;
                        let next_seq = hover_seq() + 1;
                        hover_seq.set(next_seq);
                        let input_id = input_id.clone();
                        let value = value.clone();
                        let vars = vars.clone();
                        spawn(async move {
                            let already_open = popover.peek().is_some();
                            let dwell = if already_open { HOVER_SWITCH_MS } else { HOVER_DWELL_MS };
                            tokio::time::sleep(Duration::from_millis(dwell)).await;
                            if *hover_seq.peek() != next_seq {
                                return;
                            }
                            if let Some((idx, left, top)) =
                                read_hover_index_and_anchor(&input_id, client_x).await
                            {
                                if *hover_seq.peek() != next_seq {
                                    return;
                                }
                                if find_var_at(&value, idx).is_some() {
                                    try_open_popover(
                                        &value, &vars, idx, left, top, popover, draft, dismiss_gen,
                                    );
                                } else if popover.peek().is_some() {
                                    schedule_dismiss(popover, dismiss_gen, editing, over_popover);
                                }
                            }
                        });
                    }
                },
            }

            if let Some(menu) = var_suggest() {
                VarSuggestDropdown {
                    menu: menu.clone(),
                    on_pick: {
                        let open = menu.open_at;
                        let input_id = input_id.clone();
                        let value = value.clone();
                        move |item: VariableSuggestion| {
                            let input_id = input_id.clone();
                            let value = value.clone();
                            let item = item.clone();
                            spawn(async move {
                                if let Some((caret, _, _)) = read_field_caret_anchor(&input_id).await {
                                    let (new_text, new_caret) =
                                        apply_var_insert(&value, caret, open, &item.insert);
                                    on_input.call(new_text);
                                    restore_field_caret(&input_id, new_caret).await;
                                }
                            });
                            var_suggest.set(None);
                        }
                    },
                    on_dismiss: move |_| var_suggest.set(None),
                }
            }

            if let Some(state) = popover() {
                {
                    let info = state.info.clone();
                    let scope_label = info.scope.label();
                    let left = state.left;
                    let top = state.top;
                    let token = if info.scope == VarResolveScope::Path {
                        format!(":{}", info.name)
                    } else {
                        format!("{{{{{}}}}}", info.name)
                    };
                    rsx! {
                        DismissBackdrop {
                            z_index: 940,
                            on_dismiss: move |_| {
                                dismiss_gen.set(dismiss_gen() + 1);
                                editing.set(false);
                                popover.set(None);
                            },
                        }
                        div {
                            class: "var-popover",
                            style: "left: {left}px; top: {top}px;",
                            onmouseenter: move |_| {
                                over_popover.set(true);
                                cancel_dismiss(dismiss_gen);
                            },
                            onmouseleave: move |_| {
                                over_popover.set(false);
                                schedule_dismiss(popover, dismiss_gen, editing, over_popover);
                            },
                            onkeydown: {
                                let info = info.clone();
                                move |evt: Event<KeyboardData>| {
                                    match evt.key() {
                                        Key::Escape => {
                                            evt.prevent_default();
                                            dismiss_gen.set(dismiss_gen() + 1);
                                            editing.set(false);
                                            popover.set(None);
                                        }
                                        Key::Enter => {
                                            evt.prevent_default();
                                            let value = draft();
                                            if info.is_unresolved() {
                                                on_var_edit.call(VarEditEvent::AddToEnvironment {
                                                    name: info.name.clone(),
                                                    value,
                                                });
                                            } else {
                                                on_var_edit.call(VarEditEvent::Save {
                                                    name: info.name.clone(),
                                                    raw: info.raw.clone(),
                                                    scope: info.scope,
                                                    value,
                                                });
                                            }
                                            editing.set(false);
                                            popover.set(None);
                                        }
                                        _ => {}
                                    }
                                }
                            },
                            div { class: "var-popover-head",
                                code { class: "var-popover-name", "{token}" }
                                span {
                                    class: if info.is_unresolved() {
                                        "var-popover-scope unresolved"
                                    } else {
                                        "var-popover-scope"
                                    },
                                    "{scope_label}"
                                }
                            }
                            if info.is_unresolved() {
                                p { class: "hint var-popover-hint",
                                    "Unresolved — add to active environment or dismiss."
                                }
                            }
                            label { class: "hint", "Value" }
                            input {
                                class: "var-popover-input",
                                r#type: "text",
                                value: "{draft()}",
                                autofocus: true,
                                onfocus: move |_| {
                                    editing.set(true);
                                    cancel_dismiss(dismiss_gen);
                                },
                                onblur: move |_| {
                                    editing.set(false);
                                    schedule_dismiss(popover, dismiss_gen, editing, over_popover);
                                },
                                oninput: move |evt| draft.set(evt.value()),
                            }
                            div { class: "var-popover-actions",
                                if info.is_unresolved() {
                                    button {
                                        class: "btn btn-send",
                                        onclick: {
                                            let info = info.clone();
                                            move |_| {
                                                on_var_edit.call(VarEditEvent::AddToEnvironment {
                                                    name: info.name.clone(),
                                                    value: draft(),
                                                });
                                                editing.set(false);
                                                popover.set(None);
                                            }
                                        },
                                        "Add to environment"
                                    }
                                } else {
                                    button {
                                        class: "btn btn-send",
                                        onclick: {
                                            let info = info.clone();
                                            move |_| {
                                                on_var_edit.call(VarEditEvent::Save {
                                                    name: info.name.clone(),
                                                    raw: info.raw.clone(),
                                                    scope: info.scope,
                                                    value: draft(),
                                                });
                                                editing.set(false);
                                                popover.set(None);
                                            }
                                        },
                                        "Save"
                                    }
                                }
                                button {
                                    class: "btn btn-ghost",
                                    onclick: move |_| {
                                        editing.set(false);
                                        popover.set(None);
                                    },
                                    "Esc"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn sync_mirror_scroll(input_id: &str) {
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
        id = serde_json::to_string(input_id).unwrap_or_else(|_| "\"\"".into())
    );
    let _ = document::eval(&script).await;
}

async fn read_caret_and_anchor(input_id: &str) -> Option<(usize, f64, f64)> {
    let script = format!(
        r#"
        const el = document.getElementById({id});
        if (!el) return null;
        const rect = el.getBoundingClientRect();
        const host = el.closest('.var-hover-host');
        const hr = host ? host.getBoundingClientRect() : rect;
        const caretChars = el.selectionStart ?? 0;
        const byteIdx = new TextEncoder().encode((el.value || '').slice(0, caretChars)).length;
        const boundaryEl = el.closest('.modal, .modal-dialog, .workbench-pane, #main') || document.body;
        const boundary = boundaryEl.getBoundingClientRect();
        const popWidth = 300;
        const maxGlobalRight = Math.min(window.innerWidth - 12, boundary.right - 12);
        const maxLeft = Math.max(8, maxGlobalRight - hr.left - popWidth);
        const left = Math.max(8, Math.min(8, maxLeft));
        return [byteIdx, left, rect.bottom - hr.top + 4];
        "#,
        id = serde_json::to_string(input_id).unwrap_or_else(|_| "\"\"".into())
    );
    let v = document::eval(&script).await.ok()?;
    parse_index_anchor(&v)
}

async fn read_hover_index_and_anchor(input_id: &str, client_x: f64) -> Option<(usize, f64, f64)> {
    let script = format!(
        r#"
        const el = document.getElementById({id});
        if (!el) return null;
        const style = getComputedStyle(el);
        const padL = parseFloat(style.paddingLeft) || 0;
        const rect = el.getBoundingClientRect();
        const x = {client_x} - rect.left - padL + (el.scrollLeft || 0);
        const canvas = document.createElement('canvas');
        const ctx = canvas.getContext('2d');
        ctx.font = (style.fontWeight || '400') + ' ' + (style.fontSize || '13px') + ' ' + (style.fontFamily || 'monospace');
        const text = el.value || '';
        let lo = 0, hi = text.length;
        while (lo < hi) {{
            const mid = (lo + hi + 1) >> 1;
            if (ctx.measureText(text.slice(0, mid)).width <= x) lo = mid;
            else hi = mid - 1;
        }}
        const byteIdx = new TextEncoder().encode(text.slice(0, lo)).length;
        const host = el.closest('.var-hover-host');
        const hr = host ? host.getBoundingClientRect() : rect;
        const boundaryEl = el.closest('.modal, .modal-dialog, .workbench-pane, #main') || document.body;
        const boundary = boundaryEl.getBoundingClientRect();
        const popWidth = 300;
        const maxGlobalRight = Math.min(window.innerWidth - 12, boundary.right - 12);
        const rawLeft = {client_x} - hr.left - 40;
        const maxLeft = Math.max(8, maxGlobalRight - hr.left - popWidth);
        const left = Math.max(8, Math.min(rawLeft, maxLeft));
        return [byteIdx, left, rect.bottom - hr.top + 4];
        "#,
        id = serde_json::to_string(input_id).unwrap_or_else(|_| "\"\"".into()),
        client_x = client_x
    );
    let v = document::eval(&script).await.ok()?;
    parse_index_anchor(&v)
}

fn parse_index_anchor(v: &serde_json::Value) -> Option<(usize, f64, f64)> {
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
