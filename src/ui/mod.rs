//! Requester workbench shell (reports/02 product-docs UX mapping).

mod assertions_editor;
pub(crate) mod code_editor;
mod collection_editor;
pub(crate) mod command_palette;
mod comment_toggle;
mod conflict_resolver;
mod console_panel;
mod dismiss_backdrop;
mod empty_state;
mod env_diff_view;
mod env_selector;
pub(crate) mod icons;
mod json_highlight;
mod js_highlight;
mod line_comments;
mod var_autocomplete;
mod lint_panel;
mod loading;
mod modal_shell;
mod perf_modal;
mod request_builder;
mod response_viewer;
mod runner_panel;
pub(crate) mod settings_panel;
mod theme_selector;
mod sidebar;
mod sidebar_shortcuts;
mod team_modal;
mod toast;
mod var_hover;
mod whats_new_panel;
pub(crate) mod workbench;

pub(crate) use assertions_editor::AssertionsEditor;
pub(crate) use team_modal::TeamModal;
pub(crate) use toast::{push_toast, Toast, ToastContainer, ToastLevel};

pub(crate) use collection_editor::CollectionEditor;
pub(crate) use lint_panel::LintPanel;
pub(crate) use conflict_resolver::{ConflictResolver, ConflictState};
pub(crate) use dismiss_backdrop::DismissBackdrop;
pub(crate) use empty_state::EmptyState;
pub(crate) use modal_shell::ModalShell;
pub(crate) use env_diff_view::EnvDiffView;
pub(crate) use perf_modal::PerformanceModal;
pub(crate) use whats_new_panel::{WhatsNewAction, WhatsNewPanel};

pub(crate) use code_editor::ScriptCodeEditor;
pub(crate) use line_comments::CommentableTextarea;
pub use workbench::Workbench;

/// Compute a `[start, end)` visible slice for virtualized lists when `len > threshold`.
pub(crate) fn list_window(len: usize, anchor: usize, threshold: usize, span: usize) -> (usize, usize) {
    if len <= threshold || span == 0 {
        return (0, len);
    }
    let anchor = anchor.min(len - 1);
    let end = (anchor.saturating_sub(24) + span).min(len);
    let start = end.saturating_sub(span);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::list_window;

    #[test]
    fn list_window_keeps_short_and_slices_long_lists() {
        assert_eq!(list_window(40, 20, 100, 120), (0, 40));
        let (s, e) = list_window(500, 250, 100, 120);
        assert!(s <= 250 && 250 < e);
        assert_eq!(e - s, 120);
        assert_eq!(list_window(500, 999, 100, 120), (380, 500));
    }
}
