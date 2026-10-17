//! Requester workbench shell (reports/02 product-docs UX mapping).

mod console_panel;
mod env_manager;
mod env_selector;
mod line_comments;
mod request_builder;
mod response_viewer;
mod runner_panel;
mod settings_panel;
mod sidebar;
mod workbench;

pub(crate) use line_comments::CommentableTextarea;
pub use workbench::Workbench;
