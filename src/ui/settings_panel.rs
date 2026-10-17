use dioxus::prelude::*;

use crate::core::{AppSettings, ClientCertSettings, ProxyMode, ProxySettings, UiSettings};

#[derive(Clone, PartialEq)]
pub enum SettingsEvent {
    Close,
    Save(AppSettings),
}

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

    let mode_str = match proxy_mode() {
        ProxyMode::Env => "env",
        ProxyMode::None => "none",
        ProxyMode::Custom => "custom",
    };

    rsx! {
        div { class: "modal-backdrop",
            div { class: "modal settings-modal",
                h3 { "Settings" }
                p { class: "hint", "Proxy, certificates, appearance. Data: {data_root}" }

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
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| {
                            let d = UiSettings::default();
                            ui_font.set(d.ui_font_size_px);
                            editor_font.set(d.editor_font_size_px);
                            sidebar_w.set(d.sidebar_width_px);
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

                div { class: "modal-actions",
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| on_event.call(SettingsEvent::Close),
                        "Cancel"
                    }
                    button {
                        class: "btn btn-send",
                        onclick: move |_| {
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
                                }
                                .clamped(),
                            }));
                        },
                        "Save"
                    }
                }
            }
        }
    }
}
