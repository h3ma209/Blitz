use std::time::Duration;
use dioxus::prelude::*;
use uuid::Uuid;

#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastLevel {
    Info,
    Success,
    Warn,
    Error,
}

#[allow(dead_code)]
#[derive(Clone, PartialEq, Debug)]
pub struct Toast {
    pub id: Uuid,
    pub message: String,
    pub level: ToastLevel,
}

#[allow(dead_code)]
impl Toast {
    pub fn new(message: impl Into<String>, level: ToastLevel) -> Self {
        Self {
            id: Uuid::new_v4(),
            message: message.into(),
            level,
        }
    }

    pub fn success(msg: impl Into<String>) -> Self {
        Self::new(msg, ToastLevel::Success)
    }

    pub fn info(msg: impl Into<String>) -> Self {
        Self::new(msg, ToastLevel::Info)
    }

    pub fn warn(msg: impl Into<String>) -> Self {
        Self::new(msg, ToastLevel::Warn)
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self::new(msg, ToastLevel::Error)
    }
}

pub fn push_toast(mut toasts: Signal<Vec<Toast>>, toast: Toast) {
    let id = toast.id;
    let mut list = toasts();
    list.push(toast);
    toasts.set(list);

    spawn(async move {
        tokio::time::sleep(Duration::from_millis(2800)).await;
        let mut list = toasts();
        list.retain(|t| t.id != id);
        toasts.set(list);
    });
}

#[component]
pub fn ToastContainer(toasts: Signal<Vec<Toast>>) -> Element {
    let active_toasts = toasts();
    if active_toasts.is_empty() {
        return rsx! {};
    }

    rsx! {
        div { class: "toast-container", "aria-live": "polite",
            for t in active_toasts {
                {
                    let tid = t.id;
                    let (icon, cls) = match t.level {
                        ToastLevel::Success => ("✓", "toast-success"),
                        ToastLevel::Info => ("ℹ", "toast-info"),
                        ToastLevel::Warn => ("⚠", "toast-warn"),
                        ToastLevel::Error => ("✕", "toast-error"),
                    };
                    rsx! {
                        div {
                            key: "{tid}",
                            class: "toast {cls}",
                            span { class: "toast-icon", "{icon}" }
                            span { class: "toast-message", "{t.message}" }
                            button {
                                class: "toast-close",
                                title: "Dismiss",
                                onclick: move |_| {
                                    let mut list = toasts();
                                    list.retain(|item| item.id != tid);
                                    toasts.set(list);
                                },
                                "×"
                            }
                        }
                    }
                }
            }
        }
    }
}
