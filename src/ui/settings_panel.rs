use dioxus::prelude::*;

use crate::core::{
    AccentIntensity, AppSettings, ClientCertSettings, CloudSyncSettings, DefaultResponseTab,
    ProxyMode, ProxySettings, UiDensity, UiSettings, UiTheme, WorkbenchLayout, DEFAULT_CLOUD_REMOTE,
};

#[derive(Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum SettingsEvent {
    Close,
    Save(AppSettings),
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Appearance,
    Network,
    Certificates,
    CloudSync,
}

/// Full-page settings editor with modern category sidebar, segmented cards, and clean controls.
#[component]
pub fn SettingsPanel(
    settings: AppSettings,
    data_root: String,
    on_event: EventHandler<SettingsEvent>,
) -> Element {
    let mut active_tab = use_signal(|| SettingsTab::Appearance);

    let mut proxy_mode = use_signal(|| settings.proxy.mode);
    let mut host_port = use_signal(|| settings.proxy.host_port.clone());
    let mut cert_path = use_signal(|| settings.client_cert.cert_path.clone());
    let mut cert_pass = use_signal(|| settings.client_cert.passphrase.clone());
    let mut cert_enabled = use_signal(|| settings.client_cert.enabled);
    let mut zoom = use_signal(|| settings.ui.zoom_percent);
    let mut ui_font = use_signal(|| settings.ui.ui_font_size_px);
    let mut editor_font = use_signal(|| settings.ui.editor_font_size_px);
    let mut sidebar_w = use_signal(|| settings.ui.sidebar_width_px);
    let mut request_h = use_signal(|| settings.ui.request_pane_height_px);
    let mut console_h = use_signal(|| settings.ui.console_height_px);
    let mut toolbar_h = use_signal(|| settings.ui.toolbar_height_px);
    let mut density = use_signal(|| settings.ui.density);
    let mut wrap_editor = use_signal(|| settings.ui.wrap_editor_text);
    let mut tab_size = use_signal(|| settings.ui.tab_size);
    let mut theme = use_signal(|| settings.ui.theme);
    let mut accent = use_signal(|| settings.ui.accent_intensity);
    let mut reduce_motion = use_signal(|| settings.ui.reduce_motion);
    let mut show_status = use_signal(|| settings.ui.show_status_bar);
    let mut default_resp_tab = use_signal(|| settings.ui.default_response_tab);
    let mut workbench_layout = use_signal(|| settings.ui.layout);
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
        UiTheme::NotPostman => "not_postman",
        UiTheme::Obsidian => "obsidian",
        UiTheme::Blueprint => "blueprint",
        UiTheme::BlueprintLight => "blueprint_light",
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
                div { class: "settings-header-meta",
                    h3 { "Settings & Preferences" }
                    span { class: "settings-data-path", title: "{data_root}", "Data: {data_root}" }
                }
                button {
                    class: "modal-close",
                    r#type: "button",
                    title: "Back to workbench",
                    aria_label: "Close settings",
                    onclick: move |_| on_event.call(SettingsEvent::Close),
                    "✕"
                }
            }

            div { class: "settings-layout",
                // Left Navigation Sidebar
                aside { class: "settings-nav",
                    button {
                        class: if active_tab() == SettingsTab::Appearance { "settings-nav-item active" } else { "settings-nav-item" },
                        onclick: move |_| active_tab.set(SettingsTab::Appearance),
                        span { class: "settings-nav-icon", "🎨" }
                        div { class: "settings-nav-text",
                            span { class: "settings-nav-title", "Appearance" }
                            span { class: "settings-nav-desc", "Theme, fonts & layout" }
                        }
                    }
                    button {
                        class: if active_tab() == SettingsTab::Network { "settings-nav-item active" } else { "settings-nav-item" },
                        onclick: move |_| active_tab.set(SettingsTab::Network),
                        span { class: "settings-nav-icon", "🌐" }
                        div { class: "settings-nav-text",
                            span { class: "settings-nav-title", "Network & Proxy" }
                            span { class: "settings-nav-desc", "HTTP proxy settings" }
                        }
                    }
                    button {
                        class: if active_tab() == SettingsTab::Certificates { "settings-nav-item active" } else { "settings-nav-item" },
                        onclick: move |_| active_tab.set(SettingsTab::Certificates),
                        span { class: "settings-nav-icon", "🔒" }
                        div { class: "settings-nav-text",
                            span { class: "settings-nav-title", "Certificates" }
                            span { class: "settings-nav-desc", "Client PEM identities" }
                        }
                    }
                    button {
                        class: if active_tab() == SettingsTab::CloudSync { "settings-nav-item active" } else { "settings-nav-item" },
                        onclick: move |_| active_tab.set(SettingsTab::CloudSync),
                        span { class: "settings-nav-icon", "☁️" }
                        div { class: "settings-nav-text",
                            span { class: "settings-nav-title", "Git Cloud Sync" }
                            span { class: "settings-nav-desc", "Team sync & remote" }
                        }
                    }
                }

                // Main Content Body
                div { class: "settings-page-body",
                    match active_tab() {
                        SettingsTab::Appearance => rsx! {
                            div { class: "settings-card",
                                div { class: "settings-card-head",
                                    h4 { "Theme & Color Scheme" }
                                    p { class: "hint", "Select UI color palette and green accent vibrance." }
                                }
                                div { class: "settings-grid-2",
                                    div { class: "settings-field",
                                        label { "Theme" }
                                        select {
                                            value: "{theme_str}",
                                            onchange: move |evt: Event<FormData>| {
                                                theme.set(match evt.value().as_str() {
                                                    "not_postman" => UiTheme::NotPostman,
                                                    "obsidian" => UiTheme::Obsidian,
                                                    "blueprint" => UiTheme::Blueprint,
                                                    "blueprint_light" => UiTheme::BlueprintLight,
                                                    "slate" => UiTheme::Slate,
                                                    "light" => UiTheme::Light,
                                                    "high_contrast" => UiTheme::HighContrast,
                                                    _ => UiTheme::Spotify,
                                                });
                                            },
                                            option { value: "spotify", "Spotify (Dark & Green)" }
                                            option { value: "not_postman", "notPostman (Dark & Orange)" }
                                            option { value: "obsidian", "Obsidian Ember (Dark & Amber)" }
                                            option { value: "blueprint", "Hardware Blueprint (Industrial & Tactile)" }
                                            option { value: "blueprint_light", "Hardware Blueprint Light (Drafting Paper)" }
                                            option { value: "slate", "Dark Slate" }
                                            option { value: "light", "Light Surface" }
                                            option { value: "high_contrast", "High Contrast" }
                                        }
                                    }
                                    div { class: "settings-field",
                                        label { "Accent Intensity" }
                                        select {
                                            value: "{accent_str}",
                                            onchange: move |evt: Event<FormData>| {
                                                accent.set(match evt.value().as_str() {
                                                    "soft" => AccentIntensity::Soft,
                                                    "strong" => AccentIntensity::Strong,
                                                    _ => AccentIntensity::Normal,
                                                });
                                            },
                                            option { value: "soft", "Subtle Soft" }
                                            option { value: "normal", "Standard" }
                                            option { value: "strong", "High Vibrance" }
                                        }
                                    }
                                }
                            }

                            div { class: "settings-card",
                                div { class: "settings-card-head",
                                    h4 { "Typography & Zoom" }
                                    p { class: "hint", "Adjust zoom and font sizes. Tip: Use ⌘+ / ⌘- in workbench to zoom UI (⌘0 resets)." }
                                }
                                div { class: "settings-slider-group",
                                    div { class: "settings-slider-row",
                                        div { class: "slider-label-wrap",
                                            span { class: "slider-name", "UI Zoom" }
                                            span { class: "slider-badge", "{zoom()}%" }
                                        }
                                        input {
                                            r#type: "range",
                                            min: "{UiSettings::ZOOM_MIN_PERCENT}",
                                            max: "{UiSettings::ZOOM_MAX_PERCENT}",
                                            step: "{UiSettings::ZOOM_STEP_PERCENT}",
                                            value: "{zoom()}",
                                            oninput: move |evt| {
                                                if let Ok(v) = evt.value().parse::<u32>() {
                                                    zoom.set(UiSettings::clamp_zoom_percent(v));
                                                }
                                            },
                                        }
                                    }
                                    div { class: "settings-slider-row",
                                        div { class: "slider-label-wrap",
                                            span { class: "slider-name", "UI Font Size" }
                                            span { class: "slider-badge", "{ui_font()} px" }
                                        }
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
                                    div { class: "settings-slider-row",
                                        div { class: "slider-label-wrap",
                                            span { class: "slider-name", "Editor Font Size" }
                                            span { class: "slider-badge", "{editor_font()} px" }
                                        }
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
                                }
                            }

                            div { class: "settings-card",
                                div { class: "settings-card-head",
                                    h4 { "Dimensions & Panes" }
                                    p { class: "hint", "Customize default widths and heights for workbench panels." }
                                }
                                div { class: "settings-slider-group",
                                    div { class: "settings-slider-row",
                                        div { class: "slider-label-wrap",
                                            span { class: "slider-name", "Sidebar Width" }
                                            span { class: "slider-badge", "{sidebar_w()} px" }
                                        }
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
                                    div { class: "settings-slider-row",
                                        div { class: "slider-label-wrap",
                                            span { class: "slider-name", "Request Pane Height" }
                                            span { class: "slider-badge", "{request_h()} px" }
                                        }
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
                                    div { class: "settings-slider-row",
                                        div { class: "slider-label-wrap",
                                            span { class: "slider-name", "Console Height" }
                                            span { class: "slider-badge", "{console_h()} px" }
                                        }
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
                                    div { class: "settings-slider-row",
                                        div { class: "slider-label-wrap",
                                            span { class: "slider-name", "Toolbar Height" }
                                            span { class: "slider-badge", "{toolbar_h()} px" }
                                        }
                                        input {
                                            r#type: "range",
                                            min: "{UiSettings::TOOLBAR_MIN_PX}",
                                            max: "{UiSettings::TOOLBAR_MAX_PX}",
                                            value: "{toolbar_h()}",
                                            oninput: move |evt| {
                                                if let Ok(v) = evt.value().parse::<u32>() {
                                                    toolbar_h.set(UiSettings::clamp_toolbar_height(v));
                                                }
                                            },
                                        }
                                    }
                                    div { class: "settings-slider-row",
                                        div { class: "slider-label-wrap",
                                            span { class: "slider-name", "Splash Hold Duration" }
                                            span { class: "slider-badge", "{splash_ms()} ms" }
                                        }
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
                                }
                            }

                            div { class: "settings-card",
                                div { class: "settings-card-head",
                                    h4 { "Editor & Layout Behavior" }
                                    p { class: "hint", "Editor wrapping, indentation, padding density, and defaults." }
                                }
                                div { class: "settings-grid-2",
                                    div { class: "settings-field",
                                        label { "Padding Density" }
                                        select {
                                            value: "{density_str}",
                                            onchange: move |evt: Event<FormData>| {
                                                density.set(match evt.value().as_str() {
                                                    "compact" => UiDensity::Compact,
                                                    _ => UiDensity::Comfortable,
                                                });
                                            },
                                            option { value: "comfortable", "Comfortable (Standard)" }
                                            option { value: "compact", "Compact (Dense)" }
                                        }
                                    }
                                    div { class: "settings-field",
                                        label { "Editor Tab Size" }
                                        select {
                                            value: "{tab_size_str}",
                                            onchange: move |evt: Event<FormData>| {
                                                tab_size.set(UiSettings::clamp_tab_size(
                                                    evt.value().parse().unwrap_or(2),
                                                ));
                                            },
                                            option { value: "2", "2 Spaces" }
                                            option { value: "4", "4 Spaces" }
                                        }
                                    }
                                    div { class: "settings-field",
                                        label { "Default Response Tab" }
                                        select {
                                            value: "{resp_tab_str}",
                                            onchange: move |evt: Event<FormData>| {
                                                default_resp_tab.set(match evt.value().as_str() {
                                                    "raw" => DefaultResponseTab::Raw,
                                                    "headers" => DefaultResponseTab::Headers,
                                                    _ => DefaultResponseTab::Pretty,
                                                });
                                            },
                                            option { value: "pretty", "Pretty (Formatted)" }
                                            option { value: "raw", "Raw Text" }
                                            option { value: "headers", "Response Headers" }
                                        }
                                    }
                                    div { class: "settings-field",
                                        label { "Workbench Split Layout" }
                                        select {
                                            value: match workbench_layout() {
                                                WorkbenchLayout::Column => "column",
                                                WorkbenchLayout::Row => "row",
                                            },
                                            onchange: move |evt: Event<FormData>| {
                                                workbench_layout.set(match evt.value().as_str() {
                                                    "row" => WorkbenchLayout::Row,
                                                    _ => WorkbenchLayout::Column,
                                                });
                                            },
                                            option { value: "column", "Stacked (Top / Bottom)" }
                                            option { value: "row", "Side-by-Side (Left / Right)" }
                                        }
                                    }
                                }

                                div { class: "settings-toggles-list",
                                    label { class: "settings-toggle-row",
                                        input {
                                            r#type: "checkbox",
                                            checked: wrap_editor(),
                                            onchange: move |evt| wrap_editor.set(evt.checked()),
                                        }
                                        div { class: "toggle-meta",
                                            span { class: "toggle-label", "Soft-wrap editor text" }
                                            span { class: "toggle-desc", "Wrap long lines in body, script editors, and preview" }
                                        }
                                    }
                                    label { class: "settings-toggle-row",
                                        input {
                                            r#type: "checkbox",
                                            checked: reduce_motion(),
                                            onchange: move |evt| reduce_motion.set(evt.checked()),
                                        }
                                        div { class: "toggle-meta",
                                            span { class: "toggle-label", "Reduce UI motion" }
                                            span { class: "toggle-desc", "Disable subtle transition animations and slide effects" }
                                        }
                                    }
                                    label { class: "settings-toggle-row",
                                        input {
                                            r#type: "checkbox",
                                            checked: show_status(),
                                            onchange: move |evt| show_status.set(evt.checked()),
                                        }
                                        div { class: "toggle-meta",
                                            span { class: "toggle-label", "Show bottom status bar" }
                                            span { class: "toggle-desc", "Display active mock ports, background sync state, and messages" }
                                        }
                                    }
                                }

                                div { class: "settings-card-foot",
                                    button {
                                        class: "btn btn-ghost",
                                        onclick: move |_| {
                                            let d = UiSettings::default();
                                            zoom.set(d.zoom_percent);
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
                                            toolbar_h.set(d.toolbar_height_px);
                                        },
                                        "Reset appearance to defaults"
                                    }
                                }
                            }
                        },

                        SettingsTab::Network => rsx! {
                            div { class: "settings-card",
                                div { class: "settings-card-head",
                                    h4 { "HTTP Proxy" }
                                    p { class: "hint", "Route outbound HTTP/HTTPS requests through an HTTP proxy or environment variables." }
                                }
                                div { class: "settings-field",
                                    label { "Proxy Mode" }
                                    select {
                                        value: "{mode_str}",
                                        onchange: move |evt: Event<FormData>| {
                                            proxy_mode.set(match evt.value().as_str() {
                                                "none" => ProxyMode::None,
                                                "custom" => ProxyMode::Custom,
                                                _ => ProxyMode::Env,
                                            });
                                        },
                                        option { value: "env", "System Environment (HTTP_PROXY / HTTPS_PROXY / ALL_PROXY)" }
                                        option { value: "none", "Direct Connection (No Proxy)" }
                                        option { value: "custom", "Custom Host & Port" }
                                    }
                                }
                                if proxy_mode() == ProxyMode::Custom {
                                    div { class: "settings-field",
                                        label { "Custom Proxy Address" }
                                        input {
                                            r#type: "text",
                                            placeholder: "127.0.0.1:8080 or http://proxy.corp.internal:3128",
                                            value: "{host_port}",
                                            oninput: move |evt| host_port.set(evt.value()),
                                        }
                                        p { class: "hint", "Supports host:port or full url with optional basic authentication credentials." }
                                    }
                                }
                            }
                        },

                        SettingsTab::Certificates => rsx! {
                            div { class: "settings-card",
                                div { class: "settings-card-head",
                                    h4 { "Client Certificates (mTLS)" }
                                    p { class: "hint", "Authenticate to corporate or protected endpoints using client-side certificates." }
                                }
                                label { class: "settings-toggle-row",
                                    input {
                                        r#type: "checkbox",
                                        checked: cert_enabled(),
                                        onchange: move |evt| cert_enabled.set(evt.checked()),
                                    }
                                    div { class: "toggle-meta",
                                        span { class: "toggle-label", "Enable client certificate authentication" }
                                        span { class: "toggle-desc", "Attach PEM identity certificate and private key to requests" }
                                    }
                                }
                                div { class: "settings-field",
                                    label { "PEM Certificate File Path" }
                                    input {
                                        r#type: "text",
                                        placeholder: "{data_root}/certs/client.pem",
                                        value: "{cert_path}",
                                        oninput: move |evt| cert_path.set(evt.value()),
                                    }
                                }
                                div { class: "settings-field",
                                    label { "Certificate Passphrase" }
                                    input {
                                        r#type: "password",
                                        placeholder: "Passphrase (optional for unencrypted PEM)",
                                        value: "{cert_pass}",
                                        oninput: move |evt| cert_pass.set(evt.value()),
                                    }
                                }
                                div { class: "settings-callout",
                                    span { class: "callout-icon", "💡" }
                                    p { class: "hint",
                                        "PEM cert + key bundled in a single file is automatically wired when enabled. For encrypted secrets and tokens, use the local Vault ({data_root}/vault.json) via {{{{vault:key}}}}."
                                    }
                                }
                            }
                        },

                        SettingsTab::CloudSync => rsx! {
                            div { class: "settings-card",
                                div { class: "settings-card-head",
                                    h4 { "Git Cloud Sync" }
                                    p { class: "hint", "Sync collections and environment definitions with your team via a remote Git repository." }
                                }
                                label { class: "settings-toggle-row",
                                    input {
                                        r#type: "checkbox",
                                        checked: cloud_enabled(),
                                        onchange: move |evt| cloud_enabled.set(evt.checked()),
                                    }
                                    div { class: "toggle-meta",
                                        span { class: "toggle-label", "Enable Git Cloud Synchronization" }
                                        span { class: "toggle-desc", "Saving commits changes locally; Sync button in top bar pulls and pushes to team branch" }
                                    }
                                }
                                div { class: "settings-field",
                                    label { "Remote Git Repository URL" }
                                    input {
                                        r#type: "text",
                                        placeholder: "{DEFAULT_CLOUD_REMOTE}",
                                        value: "{cloud_remote}",
                                        oninput: move |evt| cloud_remote.set(evt.value()),
                                    }
                                    p { class: "hint", "Default repository or your team's Git/GitHub workspace repository." }
                                }
                                div { class: "settings-field",
                                    label { "Local Clone Directory Path" }
                                    input {
                                        r#type: "text",
                                        placeholder: "{default_clone_hint}",
                                        value: "{cloud_clone}",
                                        oninput: move |evt| cloud_clone.set(evt.value()),
                                    }
                                    p { class: "hint", "Leave blank to use default application cloud directory." }
                                }
                                div { class: "settings-field",
                                    label { "Commit Message Override" }
                                    input {
                                        r#type: "text",
                                        placeholder: "Auto-generated timestamp commit message (leave empty)",
                                        value: "{cloud_commit_msg}",
                                        oninput: move |evt| cloud_commit_msg.set(evt.value()),
                                    }
                                }
                                if !cloud_last_branch.is_empty() || !cloud_last_status.is_empty() {
                                    div { class: "settings-callout",
                                        span { class: "action-icon action-icon-branch", style: "margin-right: 6px;", aria_hidden: "true" }
                                        p { class: "hint",
                                            "Current sync branch: {cloud_last_branch} · Status: {cloud_last_status}"
                                        }
                                    }
                                }
                            }
                        },
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
                                layout: workbench_layout(),
                                splash_min_ms: splash_ms(),
                                zoom_percent: zoom(),
                                history_collapsed,
                                toolbar_height_px: toolbar_h(),
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
                    "Save Changes"
                }
            }
        }
    }
}
