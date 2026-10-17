use dioxus::prelude::*;

use crate::core::{AppSettings, ClientCertSettings, ProxyMode, ProxySettings};

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

    let mode_str = match proxy_mode() {
        ProxyMode::Env => "env",
        ProxyMode::None => "none",
        ProxyMode::Custom => "custom",
    };

    rsx! {
        div { class: "modal-backdrop",
            div { class: "modal settings-modal",
                h3 { "Settings" }
                p { class: "hint", "Proxy + client certificates. Data: {data_root}" }

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
                            }));
                        },
                        "Save"
                    }
                }
            }
        }
    }
}
