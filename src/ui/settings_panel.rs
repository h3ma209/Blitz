use dioxus::prelude::*;

use crate::core::{
    AccentIntensity, AppSettings, ClientCertSettings, CloudSyncSettings, DefaultResponseTab,
    ProxyMode, ProxySettings, UiDensity, UiSettings, UiTheme, DEFAULT_CLOUD_REMOTE,
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
    Workspace,
}

/// Full-page settings editor with modern category sidebar, segmented cards, and clean controls.
#[component]
pub fn SettingsPanel(
    settings: ReadSignal<AppSettings>,
    data_root: ReadSignal<String>,
    mut active_tab: Signal<SettingsTab>,
    on_event: EventHandler<SettingsEvent>,
) -> Element {
    let s_now = settings();
    let data_root_show = data_root();
    let mut last_prop = use_signal(|| s_now.clone());

    let mut proxy_mode = use_signal(|| s_now.proxy.mode);
    let mut host_port = use_signal(|| s_now.proxy.host_port.clone());
    let mut cert_path = use_signal(|| s_now.client_cert.cert_path.clone());
    let mut cert_pass = use_signal(|| s_now.client_cert.passphrase.clone());
    let mut cert_enabled = use_signal(|| s_now.client_cert.enabled);
    let mut zoom = use_signal(|| s_now.ui.zoom_percent);
    let mut ui_font = use_signal(|| s_now.ui.ui_font_size_px);
    let mut input_font = use_signal(|| s_now.ui.input_font_size_px);
    let mut editor_font = use_signal(|| s_now.ui.editor_font_size_px);
    let mut response_font = use_signal(|| s_now.ui.response_font_size_px);
    let mut sidebar_w = use_signal(|| s_now.ui.sidebar_width_px);
    let mut request_h = use_signal(|| s_now.ui.request_pane_height_px);
    let mut console_h = use_signal(|| s_now.ui.console_height_px);
    let mut toolbar_h = use_signal(|| s_now.ui.toolbar_height_px);
    let mut density = use_signal(|| s_now.ui.density);
    let mut wrap_editor = use_signal(|| s_now.ui.wrap_editor_text);
    let mut tab_size = use_signal(|| s_now.ui.tab_size);
    let mut theme = use_signal(|| s_now.ui.theme);
    let mut accent = use_signal(|| s_now.ui.accent_intensity);
    let mut reduce_motion = use_signal(|| s_now.ui.reduce_motion);
    let mut storage_root = use_signal(|| s_now.storage_root.clone());
    let mut storage_roots = use_signal(|| s_now.storage_roots.clone());
    let mut bookmark_draft = use_signal(String::new);
    let mut show_status = use_signal(|| s_now.ui.show_status_bar);
    let mut default_resp_tab = use_signal(|| s_now.ui.default_response_tab);
    let mut workbench_layout = use_signal(|| s_now.ui.layout);
    let mut splash_ms = use_signal(|| s_now.ui.splash_min_ms);
    let mut cloud_enabled = use_signal(|| s_now.cloud_sync.enabled);
    let mut cloud_remote = use_signal(|| s_now.cloud_sync.remote_url.clone());
    let mut cloud_clone = use_signal(|| s_now.cloud_sync.clone_path.clone());
    let mut cloud_commit_msg = use_signal(|| s_now.cloud_sync.commit_message.clone());

    use_effect(move || {
        let s = settings();
        let prev = last_prop.peek().clone();
        if s == prev {
            return;
        }
        if s.proxy.mode != prev.proxy.mode {
            proxy_mode.set(s.proxy.mode);
        }
        if s.proxy.host_port != prev.proxy.host_port {
            host_port.set(s.proxy.host_port.clone());
        }
        if s.client_cert.cert_path != prev.client_cert.cert_path {
            cert_path.set(s.client_cert.cert_path.clone());
        }
        if s.client_cert.passphrase != prev.client_cert.passphrase {
            cert_pass.set(s.client_cert.passphrase.clone());
        }
        if s.client_cert.enabled != prev.client_cert.enabled {
            cert_enabled.set(s.client_cert.enabled);
        }
        if s.ui.zoom_percent != prev.ui.zoom_percent {
            zoom.set(s.ui.zoom_percent);
        }
        if s.ui.ui_font_size_px != prev.ui.ui_font_size_px {
            ui_font.set(s.ui.ui_font_size_px);
        }
        if s.ui.input_font_size_px != prev.ui.input_font_size_px {
            input_font.set(s.ui.input_font_size_px);
        }
        if s.ui.editor_font_size_px != prev.ui.editor_font_size_px {
            editor_font.set(s.ui.editor_font_size_px);
        }
        if s.ui.response_font_size_px != prev.ui.response_font_size_px {
            response_font.set(s.ui.response_font_size_px);
        }
        if s.ui.sidebar_width_px != prev.ui.sidebar_width_px {
            sidebar_w.set(s.ui.sidebar_width_px);
        }
        if s.ui.request_pane_height_px != prev.ui.request_pane_height_px {
            request_h.set(s.ui.request_pane_height_px);
        }
        if s.ui.console_height_px != prev.ui.console_height_px {
            console_h.set(s.ui.console_height_px);
        }
        if s.ui.toolbar_height_px != prev.ui.toolbar_height_px {
            toolbar_h.set(s.ui.toolbar_height_px);
        }
        if s.ui.density != prev.ui.density {
            density.set(s.ui.density);
        }
        if s.ui.wrap_editor_text != prev.ui.wrap_editor_text {
            wrap_editor.set(s.ui.wrap_editor_text);
        }
        if s.ui.tab_size != prev.ui.tab_size {
            tab_size.set(s.ui.tab_size);
        }
        if s.ui.theme != prev.ui.theme {
            theme.set(s.ui.theme);
        }
        if s.ui.accent_intensity != prev.ui.accent_intensity {
            accent.set(s.ui.accent_intensity);
        }
        if s.ui.reduce_motion != prev.ui.reduce_motion {
            reduce_motion.set(s.ui.reduce_motion);
        }
        if s.ui.show_status_bar != prev.ui.show_status_bar {
            show_status.set(s.ui.show_status_bar);
        }
        if s.ui.default_response_tab != prev.ui.default_response_tab {
            default_resp_tab.set(s.ui.default_response_tab);
        }
        if s.ui.layout != prev.ui.layout {
            workbench_layout.set(s.ui.layout);
        }
        if s.ui.splash_min_ms != prev.ui.splash_min_ms {
            splash_ms.set(s.ui.splash_min_ms);
        }
        if s.storage_root != prev.storage_root {
            storage_root.set(s.storage_root.clone());
        }
        if s.storage_roots != prev.storage_roots {
            storage_roots.set(s.storage_roots.clone());
        }
        if s.cloud_sync.enabled != prev.cloud_sync.enabled {
            cloud_enabled.set(s.cloud_sync.enabled);
        }
        if s.cloud_sync.remote_url != prev.cloud_sync.remote_url {
            cloud_remote.set(s.cloud_sync.remote_url.clone());
        }
        if s.cloud_sync.clone_path != prev.cloud_sync.clone_path {
            cloud_clone.set(s.cloud_sync.clone_path.clone());
        }
        if s.cloud_sync.commit_message != prev.cloud_sync.commit_message {
            cloud_commit_msg.set(s.cloud_sync.commit_message.clone());
        }
        last_prop.set(s);
    });

    let cloud_last_status = s_now.cloud_sync.last_status.clone();
    let cloud_last_branch = s_now.cloud_sync.last_branch.clone();
    let default_clone_hint = format!("{data_root_show}/cloud");

    let mode_str = match proxy_mode() {
        ProxyMode::Env => "env",
        ProxyMode::None => "none",
        ProxyMode::Custom => "custom",
    };
    let density_str = match density() {
        UiDensity::Comfortable => "comfortable",
        UiDensity::Compact => "compact",
    };
    let theme_str = theme().serde_key();
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
                    span { class: "settings-data-path", title: "{data_root_show}", "Data: {data_root_show}" }
                }
                button {
                    class: "btn btn-ghost vars-close-btn",
                    r#type: "button",
                    title: "Back to workbench",
                    aria_label: "Close settings",
                    onclick: move |_| on_event.call(SettingsEvent::Close),
                    "✕ Close"
                }
            }

            div { class: "settings-layout",
                // Left Navigation Sidebar (categorized)
                aside { class: "settings-nav",
                    div { class: "settings-nav-section", "Interface" }
                    button {
                        r#type: "button",
                        class: if active_tab() == SettingsTab::Appearance { "settings-nav-item active" } else { "settings-nav-item" },
                        onclick: move |_| active_tab.set(SettingsTab::Appearance),
                        span { class: "settings-nav-icon", super::icons::ActionIcon { name: "theme", size: 18 } }
                        div { class: "settings-nav-text",
                            span { class: "settings-nav-title", "Appearance" }
                            span { class: "settings-nav-desc", "Theme, fonts & layout" }
                        }
                    }

                    div { class: "settings-nav-section", "Connectivity" }
                    button {
                        r#type: "button",
                        class: if active_tab() == SettingsTab::Network { "settings-nav-item active" } else { "settings-nav-item" },
                        onclick: move |_| active_tab.set(SettingsTab::Network),
                        span { class: "settings-nav-icon", super::icons::ActionIcon { name: "globe", size: 18 } }
                        div { class: "settings-nav-text",
                            span { class: "settings-nav-title", "Network & Proxy" }
                            span { class: "settings-nav-desc", "HTTP proxy settings" }
                        }
                    }
                    button {
                        r#type: "button",
                        class: if active_tab() == SettingsTab::Certificates { "settings-nav-item active" } else { "settings-nav-item" },
                        onclick: move |_| active_tab.set(SettingsTab::Certificates),
                        span { class: "settings-nav-icon", super::icons::ActionIcon { name: "lock", size: 18 } }
                        div { class: "settings-nav-text",
                            span { class: "settings-nav-title", "Certificates" }
                            span { class: "settings-nav-desc", "Client PEM identities" }
                        }
                    }

                    div { class: "settings-nav-section", "Storage & Sync" }
                    button {
                        r#type: "button",
                        class: if active_tab() == SettingsTab::Workspace { "settings-nav-item active" } else { "settings-nav-item" },
                        onclick: move |_| active_tab.set(SettingsTab::Workspace),
                        span { class: "settings-nav-icon", super::icons::ActionIcon { name: "folder", size: 18 } }
                        div { class: "settings-nav-text",
                            span { class: "settings-nav-title", "Workspace" }
                            span { class: "settings-nav-desc", "Data directory" }
                        }
                    }
                    button {
                        r#type: "button",
                        class: if active_tab() == SettingsTab::CloudSync { "settings-nav-item active" } else { "settings-nav-item" },
                        onclick: move |_| active_tab.set(SettingsTab::CloudSync),
                        span { class: "settings-nav-icon", super::icons::ActionIcon { name: "cloud", size: 18 } }
                        div { class: "settings-nav-text",
                            span { class: "settings-nav-title", "Git Cloud Sync" }
                            span { class: "settings-nav-desc", "Team sync & remote" }
                        }
                        span {
                            class: if cloud_enabled() { "settings-nav-badge on" } else { "settings-nav-badge" },
                            if cloud_enabled() { "ON" } else { "OFF" }
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
                                                theme.set(UiTheme::from_key(evt.value().as_str()));
                                            },
                                            for t in UiTheme::all() {
                                                option {
                                                    value: "{t.serde_key()}",
                                                    "{t.label()} — {t.description()}"
                                                }
                                            }
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
                                    {font_size_slider_row("UI Font Size", ui_font)}
                                    {font_size_slider_row("Input Areas Font Size", input_font)}
                                    {font_size_slider_row("Request Body & Script Font Size", editor_font)}
                                    {font_size_slider_row("Response Body Font Size", response_font)}
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
                                            span {
                                                class: "slider-badge",
                                                if request_h() == 0 {
                                                    "Auto (60%)"
                                                } else {
                                                    "{request_h()} px"
                                                }
                                            }
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
                                    /* Workbench Split Layout disabled for now
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
                                    */
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
                                        r#type: "button",
                                        class: "btn btn-ghost",
                                        onclick: move |_| {
                                            let d = UiSettings::default();
                                            zoom.set(d.zoom_percent);
                                            ui_font.set(d.ui_font_size_px);
                                            input_font.set(d.input_font_size_px);
                                            editor_font.set(d.editor_font_size_px);
                                            response_font.set(d.response_font_size_px);
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
                                            workbench_layout.set(d.layout);
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
                                        placeholder: "{data_root_show}/certs/client.pem",
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
                                    span { class: "callout-icon", super::icons::ActionIcon { name: "lightbulb", size: 16 } }
                                    p { class: "hint",
                                        "PEM cert + key bundled in a single file is automatically wired when enabled. For encrypted secrets and tokens, use the local Vault ({data_root_show}/vault.json) via {{{{vault:key}}}}."
                                    }
                                }
                            }
                        },

                        SettingsTab::Workspace => rsx! {
                            div { class: "settings-card",
                                div { class: "settings-card-head",
                                    h4 { "Workspace storage" }
                                    p { class: "hint", "Switch Blitz data directory (collections, envs, vault). Use project-local `.blitz` for git repos." }
                                }
                                div { class: "settings-field",
                                    label { "Active data directory" }
                                    input {
                                        r#type: "text",
                                        placeholder: "{data_root_show}",
                                        value: "{storage_root}",
                                        oninput: move |evt| storage_root.set(evt.value()),
                                    }
                                }
                                div { class: "settings-field",
                                    label { "Bookmarked workspaces" }
                                    {
                                        let roots = storage_roots();
                                        rsx! {
                                            if !roots.is_empty() {
                                                div { class: "settings-workspace-list",
                                                    for (ri, path) in roots.iter().enumerate() {
                                                        {
                                                            let path = path.clone();
                                                            let path_rm = path.clone();
                                                            rsx! {
                                                                div { key: "{ri}-{path}", class: "settings-workspace-row",
                                                                    button {
                                                                        r#type: "button",
                                                                        class: "btn btn-ghost btn-sm settings-workspace-pick",
                                                                        onclick: move |_| storage_root.set(path.clone()),
                                                                        "{path}"
                                                                    }
                                                                    button {
                                                                        r#type: "button",
                                                                        class: "btn btn-ghost btn-sm",
                                                                        title: "Remove bookmark",
                                                                        onclick: move |_| {
                                                                            let mut list = storage_roots();
                                                                            list.retain(|p| p != &path_rm);
                                                                            storage_roots.set(list);
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
                                    }
                                    div { class: "settings-inline-row",
                                        input {
                                            r#type: "text",
                                            placeholder: "Add path…",
                                            value: "{bookmark_draft}",
                                            oninput: move |evt| bookmark_draft.set(evt.value()),
                                        }
                                        button {
                                            r#type: "button",
                                            class: "btn btn-secondary btn-sm",
                                            onclick: move |_| {
                                                let p = bookmark_draft().trim().to_string();
                                                if p.is_empty() { return; }
                                                let mut list = storage_roots();
                                                if !list.iter().any(|x| x == &p) {
                                                    list.push(p);
                                                    storage_roots.set(list);
                                                }
                                                bookmark_draft.set(String::new());
                                            },
                                            "Bookmark"
                                        }
                                        button {
                                            r#type: "button",
                                            class: "btn btn-secondary btn-sm",
                                            onclick: move |_| {
                                                let root = data_root();
                                                if !root.is_empty() {
                                                    storage_root.set(root);
                                                }
                                            },
                                            "Use current"
                                        }
                                    }
                                }
                            }
                        },

                        SettingsTab::CloudSync => rsx! {
                            div { class: "cloud-sync-hero",
                                div { class: "cloud-sync-hero-main",
                                    div { class: "cloud-sync-hero-icon",
                                        super::icons::ActionIcon { name: "cloud", size: 22 }
                                    }
                                    div { class: "cloud-sync-hero-copy",
                                        div { class: "cloud-sync-hero-title-row",
                                            h4 { "Git Cloud Sync" }
                                            span {
                                                class: if cloud_enabled() { "cloud-status-pill on" } else { "cloud-status-pill" },
                                                if cloud_enabled() { "Sync Enabled" } else { "Paused" }
                                            }
                                            if !cloud_last_branch.is_empty() {
                                                span { class: "cloud-branch-chip",
                                                    super::icons::ActionIcon { name: "branch", size: 13 }
                                                    span { "{cloud_last_branch}" }
                                                }
                                            }
                                        }
                                        p { class: "hint",
                                            if !cloud_last_status.is_empty() {
                                                "Last status: {cloud_last_status}"
                                            } else {
                                                "Share collections and environment schemas with your team via Git."
                                            }
                                        }
                                    }
                                }
                                label { class: "cloud-switch",
                                    input {
                                        r#type: "checkbox",
                                        checked: cloud_enabled(),
                                        onchange: move |evt| cloud_enabled.set(evt.checked()),
                                    }
                                    span { class: "cloud-switch-label",
                                        if cloud_enabled() { "Enabled" } else { "Enable" }
                                    }
                                }
                            }

                            div { class: "settings-card",
                                div { class: "settings-card-head",
                                    h4 { "Repository & Local Clone" }
                                    p { class: "hint", "Point Blitz to your team Git repository and local working folder." }
                                }
                                div { class: "settings-field",
                                    label { "Remote Git Repository URL" }
                                    div { class: "settings-inline-row",
                                        input {
                                            r#type: "text",
                                            placeholder: "{DEFAULT_CLOUD_REMOTE}",
                                            value: "{cloud_remote}",
                                            oninput: move |evt| cloud_remote.set(evt.value()),
                                        }
                                        button {
                                            r#type: "button",
                                            class: "btn btn-secondary btn-sm",
                                            title: "Fill default team repository URL",
                                            onclick: move |_| cloud_remote.set(DEFAULT_CLOUD_REMOTE.to_string()),
                                            "Default"
                                        }
                                    }
                                    p { class: "hint", "HTTPS or SSH Git URL for your shared Blitz workspace." }
                                }
                                div { class: "settings-field",
                                    label { "Local Clone Directory" }
                                    div { class: "settings-inline-row",
                                        input {
                                            r#type: "text",
                                            placeholder: "{default_clone_hint}",
                                            value: "{cloud_clone}",
                                            oninput: move |evt| cloud_clone.set(evt.value()),
                                        }
                                        button {
                                            r#type: "button",
                                            class: "btn btn-secondary btn-sm",
                                            title: "Pick folder",
                                            onclick: move |_| {
                                                if let Some(dir) = rfd::FileDialog::new()
                                                    .set_title("Choose Git Cloud Sync clone directory")
                                                    .pick_folder()
                                                {
                                                    cloud_clone.set(dir.display().to_string());
                                                }
                                            },
                                            "Browse…"
                                        }
                                        if !cloud_clone().trim().is_empty() {
                                            button {
                                                r#type: "button",
                                                class: "btn btn-ghost btn-sm",
                                                title: "Reset to default app directory",
                                                onclick: move |_| cloud_clone.set(String::new()),
                                                "Reset"
                                            }
                                        }
                                    }
                                    p { class: "hint", "Leave blank to use default directory ({default_clone_hint})." }
                                }
                            }

                            div { class: "settings-card",
                                div { class: "settings-card-head",
                                    h4 { "Commit & Workflow" }
                                    p { class: "hint", "Customize commit messages or use smart automatic summaries." }
                                }
                                div { class: "settings-field",
                                    label { "Commit Message Override" }
                                    input {
                                        r#type: "text",
                                        placeholder: "Auto-generated from changed files (recommended)",
                                        value: "{cloud_commit_msg}",
                                        oninput: move |evt| cloud_commit_msg.set(evt.value()),
                                    }
                                    div { class: "cloud-preset-row",
                                        span { class: "cloud-preset-label", "Quick presets:" }
                                        button {
                                            r#type: "button",
                                            class: if cloud_commit_msg().trim().is_empty() { "cloud-preset-chip active" } else { "cloud-preset-chip" },
                                            onclick: move |_| cloud_commit_msg.set(String::new()),
                                            "Auto"
                                        }
                                        button {
                                            r#type: "button",
                                            class: if cloud_commit_msg().trim() == "sync: update collections" { "cloud-preset-chip active" } else { "cloud-preset-chip" },
                                            onclick: move |_| cloud_commit_msg.set("sync: update collections".to_string()),
                                            "sync: update collections"
                                        }
                                        button {
                                            r#type: "button",
                                            class: if cloud_commit_msg().trim() == "chore: update environments" { "cloud-preset-chip active" } else { "cloud-preset-chip" },
                                            onclick: move |_| cloud_commit_msg.set("chore: update environments".to_string()),
                                            "chore: update environments"
                                        }
                                    }
                                }
                                div { class: "cloud-steps-grid",
                                    div { class: "cloud-step-item",
                                        span { class: "cloud-step-num", "1" }
                                        div {
                                            div { class: "cloud-step-title", "Save Commits" }
                                            div { class: "cloud-step-desc", "Saving requests or envs snapshots clean JSON locally." }
                                        }
                                    }
                                    div { class: "cloud-step-item",
                                        span { class: "cloud-step-num", "2" }
                                        div {
                                            div { class: "cloud-step-title", "Cloud Sync" }
                                            div { class: "cloud-step-desc", "Pulls team changes, rebases, and pushes to branch." }
                                        }
                                    }
                                    div { class: "cloud-step-item",
                                        span { class: "cloud-step-num", "3" }
                                        div {
                                            div { class: "cloud-step-title", "Secrets Stay Local" }
                                            div { class: "cloud-step-desc", "Vault keys and secret env values are never pushed." }
                                        }
                                    }
                                }
                            }
                        },
                    }
                }
            }

            div { class: "settings-page-actions",
                span { class: "settings-actions-hint", "Changes take effect when you click Save Changes." }
                div { class: "settings-actions-btns",
                    button {
                        r#type: "button",
                        class: "btn btn-ghost",
                        onclick: move |_| on_event.call(SettingsEvent::Close),
                        "Cancel"
                    }
                    button {
                    r#type: "button",
                    class: "btn btn-send",
                    onclick: move |_| {
                        let cur = settings();
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
                                input_font_size_px: input_font(),
                                editor_font_size_px: editor_font(),
                                response_font_size_px: response_font(),
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
                                history_collapsed: cur.ui.history_collapsed,
                                toolbar_height_px: toolbar_h(),
                                console_log_error_payloads: cur.ui.console_log_error_payloads,
                            }
                            .clamped(),
                            cloud_sync: CloudSyncSettings {
                                enabled: cloud_enabled(),
                                remote_url: remote,
                                clone_path: cloud_clone().trim().to_string(),
                                commit_message: cloud_commit_msg().trim().to_string(),
                                last_status: cur.cloud_sync.last_status,
                                last_branch: cur.cloud_sync.last_branch,
                            },
                            storage_root: {
                                let r = storage_root().trim().to_string();
                                if r.is_empty() { data_root() } else { r }
                            },
                            storage_roots: storage_roots(),
                        }));
                    },
                    "Save Changes"
                }
                }
            }
        }
    }
}

fn font_size_slider_row(label: &str, mut value: Signal<u32>) -> Element {
    rsx! {
        div { class: "settings-slider-row",
            div { class: "slider-label-wrap",
                span { class: "slider-name", "{label}" }
                span { class: "slider-badge", "{value()} px" }
            }
            input {
                r#type: "range",
                min: "{UiSettings::FONT_MIN_PX}",
                max: "{UiSettings::FONT_MAX_PX}",
                value: "{value()}",
                oninput: move |evt| {
                    if let Ok(v) = evt.value().parse::<u32>() {
                        value.set(UiSettings::clamp_font_size(v));
                    }
                },
            }
        }
    }
}
