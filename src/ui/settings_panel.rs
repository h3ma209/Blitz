use dioxus::prelude::*;

use crate::core::{
    AccentIntensity, AppSettings, ClientCertSettings, CloudSyncSettings, DefaultResponseTab,
    ProxyMode, ProxySettings, UiDensity, UiSettings, UiTheme, DEFAULT_CLOUD_REMOTE,
};
#[derive(Clone, PartialEq)]
pub enum SettingsEvent {
    Close,
    Save(AppSettings),
}

/// Full-page settings editor (proxy, certs, appearance, cloud sync).
#[component]
pub fn SettingsPanel(
    settings: AppSettings,
    data_root: String,
    on_event: EventHandler<SettingsEvent>,
) -> Element {
    let mut proxy_mode = use_signal(|| settings.proxy.mode);
    let mut host_port = use_signal(|| settings.proxy.host_port.clone());
    let mut cert_path = use_signal(|| settings.client_cert.cert_path.clone());
    let mut cert_pass = use_signal(|| settings.client_cert.passphrase.clone());
    let mut cert_enabled = use_signal(|| settings.client_cert.enabled);
    let mut ui_font = use_signal(|| settings.ui.ui_font_size_px);
    let mut editor_font = use_signal(|| settings.ui.editor_font_size_px);
    let mut sidebar_w = use_signal(|| settings.ui.sidebar_width_px);
    let mut request_h = use_signal(|| settings.ui.request_pane_height_px);
    let mut console_h = use_signal(|| settings.ui.console_height_px);
    let mut density = use_signal(|| settings.ui.density);
    let mut wrap_editor = use_signal(|| settings.ui.wrap_editor_text);
    let mut tab_size = use_signal(|| settings.ui.tab_size);
    let mut theme = use_signal(|| settings.ui.theme);
    let mut accent = use_signal(|| settings.ui.accent_intensity);
    let mut reduce_motion = use_signal(|| settings.ui.reduce_motion);
    let mut show_status = use_signal(|| settings.ui.show_status_bar);
    let mut default_resp_tab = use_signal(|| settings.ui.default_response_tab);
    let mut splash_ms = use_signal(|| settings.ui.splash_min_ms);
    let mut cloud_enabled = use_signal(|| settings.cloud_sync.enabled);
    let mut cloud_remote = use_signal(|| settings.cloud_sync.remote_url.clone());
    let mut cloud_clone = use_signal(|| settings.cloud_sync.clone_path.clone());
    let mut cloud_commit_msg = use_signal(|| settings.cloud_sync.commit_message.clone());
    let cloud_last_status = settings.cloud_sync.last_status.clone();
    let cloud_last_branch = settings.cloud_sync.last_branch.clone();
    let history_collapsed = settings.ui.history_collapsed;
    let default_clone_hint = format!("{data_root}/cloud");

    let mode_str = match proxy_mode() {
        ProxyMode::Env => "env",
        ProxyMode::None => "none",
        ProxyMode::Custom => "custom",
    };
    let density_str = match density() {
        UiDensity::Comfortable => "comfortable",
        UiDensity::Compact => "compact",
    };
    let theme_str = match theme() {
        UiTheme::Spotify => "spotify",
        UiTheme::Slate => "slate",
        UiTheme::Light => "light",
        UiTheme::HighContrast => "high_contrast",
    };
    let accent_str = match accent() {
        AccentIntensity::Soft => "soft",
        AccentIntensity::Normal => "normal",
        AccentIntensity::Strong => "strong",
    };
    let resp_tab_str = match default_resp_tab() {
        DefaultResponseTab::Pretty => "pretty",
        DefaultResponseTab::Raw => "raw",
        DefaultResponseTab::Headers => "headers",
    };
    let tab_size_str = if tab_size() >= 4 { "4" } else { "2" };

    rsx! {
        div { class: "settings-page",
            div { class: "settings-page-head",
                h3 { "Settings" }
                button {
                    class: "modal-close",
                    r#type: "button",
                    title: "Back to workbench",
                    aria_label: "Close settings",
                    onclick: move |_| on_event.call(SettingsEvent::Close),
                    "✕"
                }
            }
            div { class: "settings-page-body",
                p { class: "hint", "Proxy, certificates, appearance, cloud sync. Data: {data_root}" }

                h4 { "Appearance" }
                div { class: "settings-fields",
                    label { class: "settings-row",
                        span { "UI font size ({ui_font()}px)" }
                        input {
                            r#type: "range",
                            min: "{UiSettings::FONT_MIN_PX}",
                            max: "{UiSettings::FONT_MAX_PX}",
                            value: "{ui_font()}",
                            oninput: move |evt| {
                                if let Ok(v) = evt.value().parse::<u32>() {
                                    ui_font.set(UiSettings::clamp_font_size(v));
                                }
                            },
                        }
                    }
                    label { class: "settings-row",
                        span { "Editor font size ({editor_font()}px)" }
                        input {
                            r#type: "range",
                            min: "{UiSettings::FONT_MIN_PX}",
                            max: "{UiSettings::FONT_MAX_PX}",
                            value: "{editor_font()}",
                            oninput: move |evt| {
                                if let Ok(v) = evt.value().parse::<u32>() {
                                    editor_font.set(UiSettings::clamp_font_size(v));
                                }
                            },
                        }
                    }
                    label { class: "settings-row",
                        span { "Sidebar width ({sidebar_w()}px)" }
                        input {
                            r#type: "range",
                            min: "{UiSettings::SIDEBAR_MIN_PX}",
                            max: "{UiSettings::SIDEBAR_MAX_PX}",
                            value: "{sidebar_w()}",
                            oninput: move |evt| {
                                if let Ok(v) = evt.value().parse::<u32>() {
                                    sidebar_w.set(UiSettings::clamp_sidebar_width(v));
                                }
                            },
                        }
                    }
                    label { class: "settings-row",
                        span { "Request pane ({request_h()}px)" }
                        input {
                            r#type: "range",
                            min: "{UiSettings::REQUEST_PANE_MIN_PX}",
                            max: "{UiSettings::REQUEST_PANE_MAX_PX}",
                            value: "{request_h()}",
                            oninput: move |evt| {
                                if let Ok(v) = evt.value().parse::<u32>() {
                                    request_h.set(UiSettings::clamp_request_pane_height(v));
                                }
                            },
                        }
                    }
                    label { class: "settings-row",
                        span { "Console height ({console_h()}px)" }
                        input {
                            r#type: "range",
                            min: "{UiSettings::CONSOLE_MIN_PX}",
                            max: "{UiSettings::CONSOLE_MAX_PX}",
                            value: "{console_h()}",
                            oninput: move |evt| {
                                if let Ok(v) = evt.value().parse::<u32>() {
                                    console_h.set(UiSettings::clamp_console_height(v));
                                }
                            },
                        }
                    }
                    label { class: "settings-row",
                        span { "Theme" }
                        select {
                            value: "{theme_str}",
                            onchange: move |evt: Event<FormData>| {
                                theme.set(match evt.value().as_str() {
                                    "slate" => UiTheme::Slate,
                                    "light" => UiTheme::Light,
                                    "high_contrast" => UiTheme::HighContrast,
                                    _ => UiTheme::Spotify,
                                });
                            },
                            option { value: "spotify", "Spotify (default)" }
                            option { value: "slate", "Dark slate" }
                            option { value: "light", "Light" }
                            option { value: "high_contrast", "High contrast" }
                        }
                    }
                    label { class: "settings-row",
                        span { "Density" }
                        select {
                            value: "{density_str}",
                            onchange: move |evt: Event<FormData>| {
                                density.set(match evt.value().as_str() {
                                    "compact" => UiDensity::Compact,
                                    _ => UiDensity::Comfortable,
                                });
                            },
                            option { value: "comfortable", "Comfortable" }
                            option { value: "compact", "Compact" }
                        }
                    }
                    label { class: "checkbox-row",
                        input {
                            r#type: "checkbox",
                            checked: wrap_editor(),
                            onchange: move |evt| wrap_editor.set(evt.checked()),
                        }
                        " Wrap editor text"
                    }
                    label { class: "settings-row",
                        span { "Tab size" }
                        select {
                            value: "{tab_size_str}",
                            onchange: move |evt: Event<FormData>| {
                                tab_size.set(UiSettings::clamp_tab_size(
                                    evt.value().parse().unwrap_or(2),
                                ));
                            },
                            option { value: "2", "2 spaces" }
                            option { value: "4", "4 spaces" }
                        }
                    }
                    label { class: "settings-row",
                        span { "Accent intensity" }
                        select {
                            value: "{accent_str}",
                            onchange: move |evt: Event<FormData>| {
                                accent.set(match evt.value().as_str() {
                                    "soft" => AccentIntensity::Soft,
                                    "strong" => AccentIntensity::Strong,
                                    _ => AccentIntensity::Normal,
                                });
                            },
                            option { value: "soft", "Soft" }
                            option { value: "normal", "Normal" }
                            option { value: "strong", "Strong" }
                        }
                    }
                    label { class: "checkbox-row",
                        input {
                            r#type: "checkbox",
                            checked: reduce_motion(),
                            onchange: move |evt| reduce_motion.set(evt.checked()),
                        }
                        " Reduce motion"
                    }
                    label { class: "checkbox-row",
                        input {
                            r#type: "checkbox",
                            checked: show_status(),
                            onchange: move |evt| show_status.set(evt.checked()),
                        }
                        " Show status bar"
                    }
                    label { class: "settings-row",
                        span { "Default response tab" }
                        select {
                            value: "{resp_tab_str}",
                            onchange: move |evt: Event<FormData>| {
                                default_resp_tab.set(match evt.value().as_str() {
                                    "raw" => DefaultResponseTab::Raw,
                                    "headers" => DefaultResponseTab::Headers,
                                    _ => DefaultResponseTab::Pretty,
                                });
                            },
                            option { value: "pretty", "Pretty" }
                            option { value: "raw", "Raw" }
                            option { value: "headers", "Headers" }
                        }
                    }
                    label { class: "settings-row",
                        span { "Splash min duration ({splash_ms()}ms)" }
                        input {
                            r#type: "range",
                            min: "{UiSettings::SPLASH_MIN_MS}",
                            max: "{UiSettings::SPLASH_MAX_MS}",
                            step: "20",
                            value: "{splash_ms()}",
                            oninput: move |evt| {
                                if let Ok(v) = evt.value().parse::<u32>() {
                                    splash_ms.set(UiSettings::clamp_splash_min_ms(v));
                                }
                            },
                        }
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| {
                            let d = UiSettings::default();
                            ui_font.set(d.ui_font_size_px);
                            editor_font.set(d.editor_font_size_px);
                            sidebar_w.set(d.sidebar_width_px);
                            request_h.set(d.request_pane_height_px);
                            console_h.set(d.console_height_px);
                            density.set(d.density);
                            wrap_editor.set(d.wrap_editor_text);
                            tab_size.set(d.tab_size);
                            theme.set(d.theme);
                            accent.set(d.accent_intensity);
                            reduce_motion.set(d.reduce_motion);
                            show_status.set(d.show_status_bar);
                            default_resp_tab.set(d.default_response_tab);
                            splash_ms.set(d.splash_min_ms);
                        },
                        "Reset appearance"
                    }
                }

                h4 { "Proxy" }
                select {
                    value: "{mode_str}",
                    onchange: move |evt: Event<FormData>| {
                        proxy_mode.set(match evt.value().as_str() {
                            "none" => ProxyMode::None,
                            "custom" => ProxyMode::Custom,
                            _ => ProxyMode::Env,
                        });
                    },
                    option { value: "env", "Use HTTP(S)_PROXY env" }
                    option { value: "none", "No proxy" }
                    option { value: "custom", "Custom host:port" }
                }
                if proxy_mode() == ProxyMode::Custom {
                    input {
                        r#type: "text",
                        placeholder: "127.0.0.1:8080",
                        value: "{host_port}",
                        oninput: move |evt| host_port.set(evt.value()),
                    }
                }

                h4 { "Client certificate" }
                label { class: "checkbox-row",
                    input {
                        r#type: "checkbox",
                        checked: cert_enabled(),
                        onchange: move |evt| cert_enabled.set(evt.checked()),
                    }
                    " Enable client cert (PEM identity)"
                }
                input {
                    r#type: "text",
                    placeholder: "{data_root}/certs/client.pem",
                    value: "{cert_path}",
                    oninput: move |evt| cert_path.set(evt.value()),
                }
                input {
                    r#type: "password",
                    placeholder: "Passphrase (unused for PEM; TODO PKCS#12)",
                    value: "{cert_pass}",
                    oninput: move |evt| cert_pass.set(evt.value()),
                }
                p { class: "hint",
                    "PEM cert+key wired when enabled. PKCS#12 / passphrase = TODO. Vault file: {data_root}/vault.json for {{{{vault:key}}}}."
                }

                h4 { "Git cloud sync" }
                label { class: "checkbox-row",
                    input {
                        r#type: "checkbox",
                        checked: cloud_enabled(),
                        onchange: move |evt| cloud_enabled.set(evt.checked()),
                    }
                    " Enable git cloud (Save → commit; Sync → pull+push)"
                }
                p { class: "hint",
                    "Environments = branches env/<name>. Save commits locally; Sync beside env shares with team. No auto-pull. Secrets / vault / cookies / history stay local."
                }
                label { class: "settings-row",
                    span { "Remote URL" }
                    input {
                        r#type: "text",
                        placeholder: "{DEFAULT_CLOUD_REMOTE}",
                        value: "{cloud_remote}",
                        oninput: move |evt| cloud_remote.set(evt.value()),
                    }
                }
                label { class: "settings-row",
                    span { "Clone path (empty = default)" }
                    input {
                        r#type: "text",
                        placeholder: "{default_clone_hint}",
                        value: "{cloud_clone}",
                        oninput: move |evt| cloud_clone.set(evt.value()),
                    }
                }
                label { class: "settings-row",
                    span { "Commit message override (empty = auto)" }
                    input {
                        r#type: "text",
                        placeholder: "2026-10-17T16:03:00Z A login.request.json …",
                        value: "{cloud_commit_msg}",
                        oninput: move |evt| cloud_commit_msg.set(evt.value()),
                    }
                }
                p { class: "hint",
                    "Also: Sync beside env (primary), or Cloud → Clone / Pull / Push / Status. Auth: `gh auth login` or SSH agent."
                }
                if !cloud_last_branch.is_empty() || !cloud_last_status.is_empty() {
                    p { class: "hint",
                        "Branch: {cloud_last_branch} · {cloud_last_status}"
                    }
                }
            }
            div { class: "settings-page-actions",
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| on_event.call(SettingsEvent::Close),
                    "Cancel"
                }
                button {
                    class: "btn btn-send",
                    onclick: move |_| {
                        let remote = {
                            let r = cloud_remote().trim().to_string();
                            if r.is_empty() {
                                DEFAULT_CLOUD_REMOTE.to_string()
                            } else {
                                r
                            }
                        };
                        on_event.call(SettingsEvent::Save(AppSettings {
                            proxy: ProxySettings {
                                mode: proxy_mode(),
                                host_port: host_port(),
                            },
                            client_cert: ClientCertSettings {
                                cert_path: cert_path(),
                                passphrase: cert_pass(),
                                enabled: cert_enabled(),
                            },
                            ui: UiSettings {
                                ui_font_size_px: ui_font(),
                                editor_font_size_px: editor_font(),
                                sidebar_width_px: sidebar_w(),
                                request_pane_height_px: request_h(),
                                console_height_px: console_h(),
                                density: density(),
                                wrap_editor_text: wrap_editor(),
                                tab_size: tab_size(),
                                theme: theme(),
                                accent_intensity: accent(),
                                reduce_motion: reduce_motion(),
                                show_status_bar: show_status(),
                                default_response_tab: default_resp_tab(),
                                splash_min_ms: splash_ms(),
                                history_collapsed,
                            }
                            .clamped(),
                            cloud_sync: CloudSyncSettings {
                                enabled: cloud_enabled(),
                                remote_url: remote,
                                clone_path: cloud_clone().trim().to_string(),
                                commit_message: cloud_commit_msg().trim().to_string(),
                                last_status: cloud_last_status.clone(),
                                last_branch: cloud_last_branch.clone(),
                            },
                        }));
                    },
                    "Save"
                }
            }
        }
    }
}
