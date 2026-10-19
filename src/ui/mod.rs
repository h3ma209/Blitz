//! Requester workbench shell (reports/02 product-docs UX mapping).

mod code_editor;
mod collection_editor;
mod comment_toggle;
mod console_panel;
mod dismiss_backdrop;
mod env_manager;
mod env_selector;
mod json_highlight;
mod js_highlight;
mod line_comments;
mod var_autocomplete;
mod loading;
mod modal_shell;
mod request_builder;
mod response_viewer;
mod runner_panel;
mod settings_panel;
mod theme_selector;
mod sidebar;
mod sidebar_shortcuts;
mod team_modal;
mod var_hover;
mod workbench;

pub(crate) use team_modal::TeamModal;

pub(crate) use collection_editor::CollectionEditor;
pub(crate) use dismiss_backdrop::DismissBackdrop;
pub(crate) use modal_shell::ModalShell;

pub(crate) use code_editor::ScriptCodeEditor;
pub(crate) use line_comments::CommentableTextarea;
pub use workbench::Workbench;
