use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::{
    fetch_oauth2_token, AuthConfig, AuthType, BodyMode, FormDataField, HttpMethod, KeyValue,
    OAuth2Grant, RequestDraft, RequestProtocol, RequestSettings, VariableContext,
};
use super::var_hover::{VarEditEvent, VarHoverInput};
use super::code_editor::{EditorSyntax, MultilineCodeField};
use super::{CommentableTextarea, DismissBackdrop, ScriptCodeEditor};

pub use super::var_hover::VarEditEvent as RequestVarEditEvent;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuilderTab {
    Message,
    Params,
    Headers,
    Body,
    Auth,
    Tests,
    Scripts,
    Docs,
    Settings,
}

#[component]
pub fn RequestBuilder(
    draft: RequestDraft,
    #[props(default)] codegen_draft: Option<RequestDraft>,
    #[props(default)] ancestor_origin: Option<crate::core::AncestorAuthOrigin>,
    #[props(default)] on_edit_parent_auth: EventHandler<uuid::Uuid>,
    on_change: EventHandler<RequestDraft>,
    on_send: EventHandler<()>,
    /// Protocol actions: ws_connect | ws_send | ws_disconnect | mqtt_publish | mqtt_subscribe | mcp_list | mcp_call
    on_protocol_action: EventHandler<String>,
    sending: bool,
    ws_connected: bool,
    #[props(default)] ws_timeline: Vec<crate::core::WsFrameItem>,
    #[props(default)] ws_auto_ping: bool,
    protocol_log: Vec<String>,
    vars: VariableContext,
    #[props(default)] data_root: String,
    #[props(default)] is_dirty: bool,
    on_var_edit: EventHandler<VarEditEvent>,
    #[props(default)] on_save: EventHandler<()>,
    #[props(default)] on_open_code: EventHandler<()>,
) -> Element {
    let default_tab = match draft.protocol {
        RequestProtocol::Http => BuilderTab::Params,
        _ => BuilderTab::Message,
    };
    let mut tab = use_signal(move || default_tab);
    let mut copied_curl = use_signal(|| false);
    let mut proto_menu_open = use_signal(|| false);
    let mut method_menu_open = use_signal(|| false);

    let proto_class = protocol_class(draft.protocol);
    let method_class = method_class_name(draft.method);

    let tab_list: Vec<(String, BuilderTab)> = match draft.protocol {
        RequestProtocol::Http => vec![
            ("Params".into(), BuilderTab::Params),
            ("Headers".into(), BuilderTab::Headers),
            ("Body".into(), BuilderTab::Body),
            ("Auth".into(), BuilderTab::Auth),
            (
                if draft.assertions.is_empty() {
                    "Tests".to_string()
                } else {
                    format!("Tests ({})", draft.assertions.iter().filter(|a| a.enabled).count())
                },
                BuilderTab::Tests,
            ),
            ("Scripts".into(), BuilderTab::Scripts),
            ("Docs".into(), BuilderTab::Docs),
            ("Settings".into(), BuilderTab::Settings),
        ],
        RequestProtocol::WebSocket => vec![
            ("Message / Deck".into(), BuilderTab::Message),
            ("Params".into(), BuilderTab::Params),
            ("Headers".into(), BuilderTab::Headers),
            ("Auth".into(), BuilderTab::Auth),
            ("Scripts".into(), BuilderTab::Scripts),
            ("Docs".into(), BuilderTab::Docs),
            ("Settings".into(), BuilderTab::Settings),
        ],
        RequestProtocol::Grpc => vec![
            ("Method & Payload".into(), BuilderTab::Message),
            ("Headers (Metadata)".into(), BuilderTab::Headers),
            ("Auth".into(), BuilderTab::Auth),
            ("Scripts".into(), BuilderTab::Scripts),
            ("Docs".into(), BuilderTab::Docs),
            ("Settings".into(), BuilderTab::Settings),
        ],
        RequestProtocol::GraphqlWs => vec![
            ("Subscription".into(), BuilderTab::Message),
            ("Params".into(), BuilderTab::Params),
            ("Headers".into(), BuilderTab::Headers),
            ("Auth".into(), BuilderTab::Auth),
            ("Scripts".into(), BuilderTab::Scripts),
            ("Docs".into(), BuilderTab::Docs),
            ("Settings".into(), BuilderTab::Settings),
        ],
        RequestProtocol::Sse => vec![
            ("Stream".into(), BuilderTab::Message),
            ("Params".into(), BuilderTab::Params),
            ("Headers".into(), BuilderTab::Headers),
            ("Auth".into(), BuilderTab::Auth),
            ("Scripts".into(), BuilderTab::Scripts),
            ("Docs".into(), BuilderTab::Docs),
            ("Settings".into(), BuilderTab::Settings),
        ],
        RequestProtocol::Mqtt => vec![
            ("Topic & Payload".into(), BuilderTab::Message),
            ("Auth".into(), BuilderTab::Auth),
            ("Scripts".into(), BuilderTab::Scripts),
            ("Docs".into(), BuilderTab::Docs),
            ("Settings".into(), BuilderTab::Settings),
        ],
        RequestProtocol::SocketIo => vec![
            ("Event & Payload".into(), BuilderTab::Message),
            ("Headers".into(), BuilderTab::Headers),
            ("Auth".into(), BuilderTab::Auth),
            ("Scripts".into(), BuilderTab::Scripts),
            ("Docs".into(), BuilderTab::Docs),
            ("Settings".into(), BuilderTab::Settings),
        ],
        RequestProtocol::Mcp => vec![
            ("Tool & Args".into(), BuilderTab::Message),
            ("Docs".into(), BuilderTab::Docs),
            ("Settings".into(), BuilderTab::Settings),
        ],
    };

    let active_tab = if tab_list.iter().any(|(_, t)| *t == tab()) {
        tab()
    } else {
        tab_list.first().map(|(_, t)| *t).unwrap_or(BuilderTab::Params)
    };

    rsx! {
        div { class: "request-row",
            if proto_menu_open() || method_menu_open() {
                DismissBackdrop {
                    z_index: 900,
                    on_dismiss: move |_| {
                        proto_menu_open.set(false);
                        method_menu_open.set(false);
                    },
                }
            }
            div { class: "picker-wrap",
                button {
                    r#type: "button",
                    class: "picker-pill protocol-pill protocol-pill-{proto_class}",
                    title: "Select protocol",
                    onclick: move |_| {
                        method_menu_open.set(false);
                        proto_menu_open.set(!proto_menu_open());
                    },
                    span { class: "picker-dot" }
                    span { class: "picker-pill-text", "{draft.protocol}" }
                    span { class: "picker-caret", "▾" }
                }
                if proto_menu_open() {
                    div { class: "picker-menu protocol-menu",
                        div { class: "picker-menu-head", "Request Protocol" }
                        for p in RequestProtocol::all() {
                            {
                                let p_val = *p;
                                let p_cls = protocol_class(p_val);
                                let (badge, desc) = protocol_meta(p_val);
                                let is_sel = p_val == draft.protocol;
                                let d_base = draft.clone();
                                rsx! {
                                    button {
                                        key: "{p_val}",
                                        r#type: "button",
                                        class: if is_sel { "picker-row active" } else { "picker-row" },
                                        onclick: move |_| {
                                            proto_menu_open.set(false);
                                            let mut d = d_base.clone();
                                            d.apply_protocol_defaults(p_val);
                                            on_change.call(d);
                                        },
                                        span { class: "picker-badge protocol-pill-{p_cls}", "{badge}" }
                                        div { class: "picker-row-info",
                                            span { class: "picker-row-title", "{p_val}" }
                                            span { class: "picker-row-sub", "{desc}" }
                                        }
                                        if is_sel {
                                            span { class: "picker-check", "✓" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if draft.protocol == RequestProtocol::Http || draft.protocol == RequestProtocol::Sse {
                div { class: "picker-wrap",
                    button {
                        r#type: "button",
                        class: "picker-pill method-pill method-pill-{method_class}",
                        title: "Select HTTP method",
                        onclick: move |_| {
                            proto_menu_open.set(false);
                            method_menu_open.set(!method_menu_open());
                        },
                        span { class: "picker-pill-text", "{draft.method}" }
                        span { class: "picker-caret", "▾" }
                    }
                    if method_menu_open() {
                        div { class: "picker-menu method-menu",
                            div { class: "picker-menu-head", "HTTP Method" }
                            for m in HttpMethod::all() {
                                {
                                    let m_val = *m;
                                    let m_cls = method_class_name(m_val);
                                    let hint = method_hint(m_val);
                                    let is_sel = m_val == draft.method;
                                    let d_base = draft.clone();
                                    rsx! {
                                        button {
                                            key: "{m_val}",
                                            r#type: "button",
                                            class: if is_sel { "picker-row active" } else { "picker-row" },
                                            onclick: move |_| {
                                                method_menu_open.set(false);
                                                let mut d = d_base.clone();
                                                d.method = m_val;
                                                on_change.call(d);
                                            },
                                            span { class: "picker-badge method-pill-{m_cls}", "{m_val}" }
                                            span { class: "picker-row-sub", "{hint}" }
                                            if is_sel {
                                                span { class: "picker-check", "✓" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            VarHoverInput {
                class: "url-input".to_string(),
                value: draft.url.clone(),
                placeholder: match draft.protocol {
                    RequestProtocol::Http => "https://api.example.com/{{path}}".to_string(),
                    RequestProtocol::WebSocket => "wss://echo.websocket.events".to_string(),
                    RequestProtocol::Grpc => "http://127.0.0.1:50051".to_string(),
                    RequestProtocol::Mqtt => "mqtt://127.0.0.1:1883".to_string(),
                    RequestProtocol::Mcp => "(STDIO — use MCP fields below)".to_string(),
                    RequestProtocol::SocketIo => "http://127.0.0.1:3000".to_string(),
                    RequestProtocol::Sse => "https://example.com/events".to_string(),
                    RequestProtocol::GraphqlWs => "ws://127.0.0.1:4000/graphql".to_string(),
                },
                input_id: "np-url-input".to_string(),
                vars: vars.clone(),
                on_input: {
                    let draft = draft.clone();
                    move |url: String| {
                        let trimmed = url.trim();
                        // Smart cURL Paste detection
                        if trimmed.starts_with("curl ")
                            || trimmed.starts_with("curl\n")
                            || trimmed.starts_with("curl\r\n")
                        {
                            if let Ok(mut imported) = crate::core::impex::import_curl(trimmed) {
                                imported.id = draft.id;
                                imported.name = draft.name.clone();
                                on_change.call(imported);
                                return;
                            }
                        }

                        let mut d = draft.clone();
                        // Auto-detect protocol schemes if currently HTTP
                        if d.protocol == RequestProtocol::Http {
                            let lower = url.trim().to_lowercase();
                            if lower.starts_with("ws://") || lower.starts_with("wss://") || lower.contains("socket_endpoint") {
                                d.apply_protocol_defaults(RequestProtocol::WebSocket);
                            } else if lower.starts_with("grpc://") || lower.starts_with("grpcs://") {
                                d.apply_protocol_defaults(RequestProtocol::Grpc);
                            } else if lower.starts_with("mqtt://") || lower.starts_with("mqtts://") {
                                d.apply_protocol_defaults(RequestProtocol::Mqtt);
                            } else if lower.starts_with("sse://") {
                                d.apply_protocol_defaults(RequestProtocol::Sse);
                            } else if lower.contains("socket.io") {
                                d.apply_protocol_defaults(RequestProtocol::SocketIo);
                            }
                        }
                        // Bi-directional URL -> Params sync
                        if url.contains('?') {
                            let (_, params) = crate::core::extract_url_query_params(&url);
                            d.params = params;
                        } else if !d.params.is_empty() && !url.ends_with('?') {
                            d.params.clear();
                        }
                        d.path_params = crate::core::sync_path_params(&url, &d.path_params);
                        d.url = url;
                        on_change.call(d);
                    }
                },
                on_var_edit,
            }
            if draft.protocol == RequestProtocol::Http || draft.protocol == RequestProtocol::Sse {
                button {
                    class: if copied_curl() { "btn btn-ghost btn-copy-curl copied" } else { "btn btn-ghost btn-copy-curl" },
                    r#type: "button",
                    style: "font-family: var(--mono); font-size: 11px; font-weight: 600; padding: 0 9px; display: inline-flex; align-items: center; gap: 4px;",
                    title: "Copy resolved request as cURL command to clipboard",
                    onclick: {
                        let draft_fallback = draft.clone();
                        let codegen_draft = codegen_draft.clone();
                        let vars = vars.clone();
                        move |_| {
                            let target_draft = codegen_draft.as_ref().unwrap_or(&draft_fallback);
                            let curl_cmd = crate::core::generate_snippet_resolved(
                                "cURL",
                                target_draft,
                                Some(&vars),
                            );
                            let script = format!(
                                "navigator.clipboard.writeText({});",
                                serde_json::to_string(&curl_cmd).unwrap_or_else(|_| "\"\"".into())
                            );
                            spawn(async move {
                                let _ = document::eval(&script).await;
                                tokio::time::sleep(std::time::Duration::from_millis(1600)).await;
                                copied_curl.set(false);
                            });
                            copied_curl.set(true);
                        }
                    },
                    if copied_curl() {
                        super::icons::ActionIcon { name: "check", size: 15 }
                        span { "Copied" }
                    } else {
                        super::icons::ActionIcon { name: "copy", size: 15 }
                        span { "cURL" }
                    }
                }
                button {
                    class: "btn btn-ghost",
                    r#type: "button",
                    style: "font-family: var(--mono); font-weight: 700; padding: 0 10px;",
                    title: "Generate Code Snippet (cURL, Python, TS, Rust…)",
                    onclick: move |_| on_open_code.call(()),
                    "</>"
                }
            }
            button {
                class: if is_dirty { "btn btn-save is-dirty" } else { "btn btn-save" },
                r#type: "button",
                title: "Save request (⌘S)",
                onclick: move |_| on_save.call(()),
                super::icons::ActionIcon { name: "save", size: 15 }
                span { "Save" }
            }
            if draft.protocol == RequestProtocol::Http || draft.protocol == RequestProtocol::Grpc {
                button {
                    class: if sending { "btn btn-ghost btn-cancel" } else { "btn btn-send" },
                    r#type: "button",
                    title: if sending { "Cancel in-flight request" } else { "Send request (⌘↵)" },
                    onclick: move |_| on_send.call(()),
                    if sending {
                        span { class: "loading-spinner-cancel" }
                        span { "Cancel" }
                    } else if draft.protocol == RequestProtocol::Grpc {
                        "Invoke"
                    } else {
                        super::icons::ActionIcon { name: "bolt", size: 15, style: "margin-right: 4px;".to_string() }
                        span { "Send" }
                    }
                }
            } else if draft.protocol == RequestProtocol::Sse {
                button {
                    class: if sending { "btn btn-ghost btn-cancel" } else { "btn btn-send" },
                    r#type: "button",
                    title: if sending { "Disconnect SSE stream" } else { "Connect & stream SSE events" },
                    onclick: move |_| {
                        if sending {
                            on_protocol_action.call("sse_stop".into());
                        } else {
                            on_protocol_action.call("sse_listen".into());
                        }
                    },
                    if sending {
                        span { class: "loading-spinner-cancel" }
                        span { "Disconnect" }
                    } else {
                        super::icons::ActionIcon { name: "bolt", size: 15, style: "margin-right: 4px;".to_string() }
                        span { "Stream" }
                    }
                }
            }
        }

        div { class: "builder-tabs",
            for (label, t) in tab_list.iter() {
                {
                    let t_val = *t;
                    rsx! {
                        button {
                            key: "{label}",
                            class: if active_tab == t_val { "builder-tab active" } else { "builder-tab" },
                            onclick: move |_| tab.set(t_val),
                            "{label}"
                        }
                    }
                }
            }
        }

        div { class: "builder-pane",
            match active_tab {
                BuilderTab::Message => rsx! {
                    ProtocolFields {
                        draft: draft.clone(),
                        on_change,
                        on_protocol_action,
                        ws_connected,
                        ws_timeline: ws_timeline.clone(),
                        ws_auto_ping,
                        protocol_log: protocol_log.clone(),
                        sending,
                        vars: Some(vars.clone()),
                    }
                },
                BuilderTab::Params => {
                    let path_vars = if draft.path_params.is_empty() {
                        crate::core::sync_path_params(&draft.url, &draft.path_params)
                    } else {
                        draft.path_params.clone()
                    };
                    rsx! {
                        div { class: "params-section",
                            if !path_vars.is_empty() {
                                div { class: "section-subtitle", "Query Params" }
                            }
                            KvEditor {
                                rows: draft.params.clone(),
                                vars: vars.clone(),
                                on_var_edit,
                                on_change: {
                                    let draft = draft.clone();
                                    move |rows| {
                                        let mut d = draft.clone();
                                        d.params = rows;
                                        d.url = crate::core::build_url_with_params(&d.url, &d.params);
                                        on_change.call(d);
                                    }
                                }
                            }
                        }
                        if !path_vars.is_empty() {
                            div { class: "params-section", style: "margin-top: 18px;",
                                div { class: "section-subtitle", "Path Variables" }
                                table { class: "kv-table",
                                    thead {
                                        tr {
                                            th { "Key" }
                                            th { "Value" }
                                        }
                                    }
                                    tbody {
                                        for (i, row) in path_vars.iter().enumerate() {
                                            tr { key: "pv-{row.key}-{i}",
                                                td {
                                                    input {
                                                        r#type: "text",
                                                        readonly: true,
                                                        value: ":{row.key}",
                                                        style: "color: #ff9800; font-weight: 600; background: transparent;",
                                                    }
                                                }
                                                td {
                                                    VarHoverInput {
                                                        class: "kv-var-input".to_string(),
                                                        value: row.value.clone(),
                                                        placeholder: "Value".to_string(),
                                                        input_id: format!("np-path-val-{i}"),
                                                        vars: vars.clone(),
                                                        on_input: {
                                                            let draft = draft.clone();
                                                            let path_vars = path_vars.clone();
                                                            move |val: String| {
                                                                let mut d = draft.clone();
                                                                let mut updated = path_vars.clone();
                                                                if let Some(r) = updated.get_mut(i) {
                                                                    r.value = val;
                                                                }
                                                                d.path_params = updated;
                                                                on_change.call(d);
                                                            }
                                                        },
                                                        on_var_edit,
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                BuilderTab::Headers => rsx! {
                    KvEditor {
                        rows: draft.headers.clone(),
                        vars: vars.clone(),
                        is_headers: true,
                        protocol: Some(draft.protocol),
                        body_mode: Some(draft.body_mode),
                        on_var_edit,
                        on_change: {
                            let draft = draft.clone();
                            move |rows| {
                                let mut d = draft.clone();
                                d.headers = rows;
                                on_change.call(d);
                            }
                        }
                    }
                },
                BuilderTab::Body => rsx! {
                    BodyEditor {
                        key: "body-{draft.id}",
                        mode: draft.body_mode,
                        body: draft.body.clone(),
                        binary_path: draft.binary_path.clone(),
                        urlencoded: draft.body_urlencoded.clone(),
                        formdata: draft.body_formdata.clone(),
                        graphql_query: draft.graphql_query.clone(),
                        graphql_variables: draft.graphql_variables.clone(),
                        vars: vars.clone(),
                        data_root: data_root.clone(),
                        on_var_edit,
                        on_change: {
                            let draft = draft.clone();
                            move |(mode, body, binary_path, urlencoded, formdata, gql_q, gql_v)| {
                                let mut d = draft.clone();
                                d.body_mode = mode;
                                d.body = body;
                                d.binary_path = binary_path;
                                d.body_urlencoded = urlencoded;
                                d.body_formdata = formdata;
                                d.graphql_query = gql_q;
                                d.graphql_variables = gql_v;
                                on_change.call(d);
                            }
                        }
                    }
                },
                BuilderTab::Auth => rsx! {
                    AuthEditor {
                        auth: draft.auth.clone(),
                        ancestor_origin: ancestor_origin.clone(),
                        vars: Some(vars.clone()),
                        on_var_edit: Some(on_var_edit),
                        on_edit_parent_auth,
                        on_change: {
                            let draft = draft.clone();
                            move |auth| {
                                let mut d = draft.clone();
                                d.auth = auth;
                                on_change.call(d);
                            }
                        }
                    }
                },
                BuilderTab::Tests => rsx! {
                    super::AssertionsEditor {
                        assertions: draft.assertions.clone(),
                        on_change: {
                            let draft = draft.clone();
                            move |list| {
                                let mut d = draft.clone();
                                d.assertions = list;
                                on_change.call(d);
                            }
                        }
                    }
                },
                BuilderTab::Scripts => rsx! {
                    ScriptsEditor {
                        key: "scripts-{draft.id}",
                        pre: draft.pre_request_script.clone(),
                        test: draft.test_script.clone(),
                        vars: vars.clone(),
                        on_change: {
                            let draft = draft.clone();
                            move |(pre, test)| {
                                let mut d = draft.clone();
                                d.pre_request_script = pre;
                                d.test_script = test;
                                on_change.call(d);
                            }
                        }
                    }
                },
                BuilderTab::Docs => rsx! {
                    div {
                        key: "docs-{draft.id}",
                        style: "display: contents;",
                        label { class: "hint", "Request description / docs" }
                        CommentableTextarea {
                            class: "body-editor".to_string(),
                            value: draft.description.clone(),
                            rows: Some("10"),
                            placeholder: "Markdown or plain text — shown for teammates / import".to_string(),
                            on_change: {
                                let draft = draft.clone();
                                move |text: String| {
                                    let mut d = draft.clone();
                                    d.description = text;
                                    on_change.call(d);
                                }
                            }
                        }
                    }
                },
                BuilderTab::Settings => rsx! {
                    RequestSettingsEditor {
                        settings: draft.settings.clone(),
                        on_change: {
                            let draft = draft.clone();
                            move |settings| {
                                let mut d = draft.clone();
                                d.settings = settings;
                                on_change.call(d);
                            }
                        }
                    }
                },
            }
        }
    }
}

fn protocol_meta(p: RequestProtocol) -> (&'static str, &'static str) {
    match p {
        RequestProtocol::Http => ("HTTP", "REST & GraphQL"),
        RequestProtocol::WebSocket => ("WS", "Bi-directional stream"),
        RequestProtocol::Grpc => ("gRPC", "Protobuf RPC"),
        RequestProtocol::Sse => ("SSE", "Server-Sent Events"),
        RequestProtocol::GraphqlWs => ("GQL", "GraphQL subscriptions"),
        RequestProtocol::Mqtt => ("MQTT", "Pub/Sub broker"),
        RequestProtocol::SocketIo => ("SIO", "Event rooms"),
        RequestProtocol::Mcp => ("MCP", "Model Context tools"),
    }
}

fn method_hint(m: HttpMethod) -> &'static str {
    match m {
        HttpMethod::Get => "Fetch resource",
        HttpMethod::Post => "Create / submit",
        HttpMethod::Put => "Replace resource",
        HttpMethod::Patch => "Partial update",
        HttpMethod::Delete => "Remove resource",
        HttpMethod::Head => "Headers only",
        HttpMethod::Options => "CORS / capabilities",
    }
}

fn protocol_class(p: RequestProtocol) -> &'static str {
    match p {
        RequestProtocol::Http => "http",
        RequestProtocol::WebSocket => "websocket",
        RequestProtocol::Grpc => "grpc",
        RequestProtocol::Mqtt => "mqtt",
        RequestProtocol::Mcp => "mcp",
        RequestProtocol::SocketIo => "socketio",
        RequestProtocol::Sse => "sse",
        RequestProtocol::GraphqlWs => "graphqlws",
    }
}

fn method_class_name(m: HttpMethod) -> &'static str {
    match m {
        HttpMethod::Get => "get",
        HttpMethod::Post => "post",
        HttpMethod::Put => "put",
        HttpMethod::Patch => "patch",
        HttpMethod::Delete => "delete",
        HttpMethod::Head => "head",
        HttpMethod::Options => "options",
    }
}

#[component]
fn ProtocolFields(
    draft: RequestDraft,
    on_change: EventHandler<RequestDraft>,
    on_protocol_action: EventHandler<String>,
    ws_connected: bool,
    #[props(default)] ws_timeline: Vec<crate::core::WsFrameItem>,
    #[props(default)] ws_auto_ping: bool,
    protocol_log: Vec<String>,
    sending: bool,
    #[props(default)] vars: Option<VariableContext>,
) -> Element {
    let mut preset_name_input = use_signal(String::new);
    let mut show_new_preset = use_signal(|| false);
    let mut selected_preset_id = use_signal(|| None::<Uuid>);
    let mut renaming_preset_id = use_signal(|| None::<Uuid>);
    let mut rename_draft = use_signal(String::new);
    let mut save_feedback = use_signal(|| false);
    let mut composer_h = use_signal(|| 140i32);
    let mut composer_drag = use_signal(|| None::<(f64, i32)>);

    let selected_preset = selected_preset_id().and_then(|id| {
        draft.ws_presets.iter().find(|p| p.id == id).cloned()
    });

    rsx! {
        div { class: "builder-pane protocol-pane",
            match draft.protocol {
                RequestProtocol::WebSocket => rsx! {
                    div { class: "ws-header-bar",
                        div { class: "ws-mode-switch",
                            button {
                                class: if draft.ws_deck_mode { "ws-mode-btn active" } else { "ws-mode-btn" },
                                onclick: {
                                    let mut d = draft.clone();
                                    move |_| {
                                        d.ws_deck_mode = true;
                                        on_change.call(d.clone());
                                    }
                                },
                                "⚡ Socket Deck"
                            }
                            button {
                                class: if !draft.ws_deck_mode { "ws-mode-btn active" } else { "ws-mode-btn" },
                                onclick: {
                                    let mut d = draft.clone();
                                    move |_| {
                                        d.ws_deck_mode = false;
                                        on_change.call(d.clone());
                                    }
                                },
                                "📄 Classic (Postman)"
                            }
                        }
                        div { class: "ws-connection-bar",
                            if !ws_connected {
                                button {
                                    class: "btn btn-send",
                                    disabled: sending,
                                    onclick: move |_| on_protocol_action.call("ws_connect".into()),
                                    "Connect"
                                }
                            } else {
                                button {
                                    class: "btn btn-danger-ghost",
                                    onclick: move |_| on_protocol_action.call("ws_disconnect".into()),
                                    "Disconnect"
                                }
                                button {
                                    class: if ws_auto_ping { "btn btn-tag active" } else { "btn btn-tag" },
                                    title: "Send heartbeat ping every 15s",
                                    onclick: move |_| on_protocol_action.call("ws_toggle_ping".into()),
                                    if ws_auto_ping { "Auto-Ping (15s)" } else { "Auto-Ping: Off" }
                                }
                                button {
                                    class: "btn btn-ghost",
                                    onclick: move |_| on_protocol_action.call("ws_clear".into()),
                                    "Clear"
                                }
                            }
                            span {
                                class: if ws_connected { "ws-state connected" } else { "ws-state disconnected" },
                                if ws_connected { "● Connected" } else { "○ Disconnected" }
                            }
                        }
                    }

                    if !draft.ws_deck_mode {
                        div { class: "ws-classic-view",
                            div { class: "body-editor-toolbar",
                                div { class: "body-editor-tools-left",
                                    button {
                                        class: "btn btn-sm btn-ghost",
                                        r#type: "button",
                                        title: "Format and indent JSON message",
                                        onclick: {
                                            let draft = draft.clone();
                                            move |_| {
                                                if let Some(pretty) = crate::core::beautify_json_body(&draft.ws_message) {
                                                    let mut d = draft.clone();
                                                    d.ws_message = pretty;
                                                    on_change.call(d);
                                                }
                                            }
                                        },
                                        super::icons::ActionIcon { name: "format", size: 15 }
                                        span { "Beautify JSON" }
                                    }
                                }
                                div { class: "body-editor-tools-right",
                                    match crate::core::lint_json_body(&draft.ws_message) {
                                        Ok(()) if !draft.ws_message.trim().is_empty() => rsx! {
                                            span { class: "json-lint-badge json-lint-ok", "✓ Valid JSON" }
                                        },
                                        Err((line, col, msg)) => rsx! {
                                            span {
                                                class: "json-lint-badge json-lint-err",
                                                title: "Line {line}, Column {col}: {msg}",
                                                "⚠ Ln {line}, Col {col}: {msg}"
                                            }
                                        },
                                        _ => rsx! {}
                                    }
                                }
                            }
                            MultilineCodeField {
                                class: "body-editor".to_string(),
                                value: draft.ws_message.clone(),
                                placeholder: "{\"type\": \"subscribe\", ...}".to_string(),
                                rows: Some("6"),
                                vars: vars.clone(),
                                syntax: EditorSyntax::Json,
                                on_change: {
                                    let draft = draft.clone();
                                    move |text: String| {
                                        let mut d = draft.clone();
                                        d.ws_message = text;
                                        on_change.call(d);
                                    }
                                },
                                on_submit: {
                                    let on_protocol_action = on_protocol_action;
                                    let ws_connected = ws_connected;
                                    Some(Callback::new(move |_| {
                                        if ws_connected {
                                            on_protocol_action.call("ws_send".into());
                                        }
                                    }))
                                },
                            }
                            div { class: "kv-actions",
                                button {
                                    class: "btn btn-ghost",
                                    disabled: !ws_connected,
                                    onclick: move |_| on_protocol_action.call("ws_send".into()),
                                    "Send message (⌘↵)"
                                }
                            }
                        }
                    } else {
                        div { class: "ws-deck-container",
                                div { class: "ws-deck-presets",
                                    div { class: "ws-deck-presets-head",
                                        span { class: "ws-deck-title", "Message Presets" }
                                        button {
                                            class: "btn btn-mini-add",
                                            title: "Add preset",
                                            onclick: move |_| {
                                                let opening = !show_new_preset();
                                                show_new_preset.set(opening);
                                                if opening {
                                                    preset_name_input.set(String::new());
                                                }
                                            },
                                            "+ Preset"
                                        }
                                    }

                                    if show_new_preset() {
                                        div { class: "ws-new-preset-box",
                                            input {
                                                class: "ws-preset-input",
                                                placeholder: "Preset Name (e.g. Subscribe)",
                                                value: "{preset_name_input}",
                                                autofocus: true,
                                                oninput: move |e: Event<FormData>| preset_name_input.set(e.value()),
                                                onkeydown: {
                                                    let draft = draft.clone();
                                                    move |e: KeyboardEvent| {
                                                        if e.key() == Key::Enter {
                                                            let name = preset_name_input().trim().to_string();
                                                            let n = if name.is_empty() { format!("Preset #{}", draft.ws_presets.len() + 1) } else { name };
                                                            let mut d = draft.clone();
                                                            let initial_msg = if d.ws_message.trim().is_empty() {
                                                                "{\n  \"action\": \"message\"\n}".to_string()
                                                            } else {
                                                                d.ws_message.clone()
                                                            };
                                                            let new_preset = crate::core::WsPreset::new(n, initial_msg);
                                                            let new_id = new_preset.id;
                                                            d.ws_presets.push(new_preset);
                                                            d.ws_message = d.ws_presets.last().unwrap().message.clone();
                                                            preset_name_input.set(String::new());
                                                            show_new_preset.set(false);
                                                            selected_preset_id.set(Some(new_id));
                                                            save_feedback.set(false);
                                                            on_change.call(d);
                                                        } else if e.key() == Key::Escape {
                                                            show_new_preset.set(false);
                                                        }
                                                    }
                                                },
                                            }
                                            div { class: "ws-new-preset-btns",
                                                button {
                                                    class: "btn btn-sm btn-send",
                                                    onclick: {
                                                        let draft = draft.clone();
                                                        move |_| {
                                                            let name = preset_name_input().trim().to_string();
                                                            let n = if name.is_empty() { format!("Preset #{}", draft.ws_presets.len() + 1) } else { name };
                                                            let mut d = draft.clone();
                                                            let initial_msg = if d.ws_message.trim().is_empty() {
                                                                "{\n  \"action\": \"message\"\n}".to_string()
                                                            } else {
                                                                d.ws_message.clone()
                                                            };
                                                            let new_preset = crate::core::WsPreset::new(n, initial_msg);
                                                            let new_id = new_preset.id;
                                                            d.ws_presets.push(new_preset);
                                                            d.ws_message = d.ws_presets.last().unwrap().message.clone();
                                                            preset_name_input.set(String::new());
                                                            show_new_preset.set(false);
                                                            selected_preset_id.set(Some(new_id));
                                                            save_feedback.set(false);
                                                            on_change.call(d);
                                                        }
                                                    },
                                                    "Add Preset"
                                                }
                                                button {
                                                    class: "btn btn-sm btn-ghost",
                                                    onclick: move |_| show_new_preset.set(false),
                                                    "Cancel"
                                                }
                                            }
                                        }
                                    }

                                    div { class: "ws-presets-list",
                                        if draft.ws_presets.is_empty() {
                                            div { class: "ws-presets-empty", "No presets yet. Click + Preset to save common messages." }
                                        }
                                        for (pi, preset) in draft.ws_presets.iter().enumerate() {
                                            {
                                                let p_id = preset.id;
                                                let p_msg = preset.message.clone();
                                                let p_name = preset.name.clone();
                                                let is_selected = selected_preset_id() == Some(p_id);
                                                let is_renaming = renaming_preset_id() == Some(p_id);
                                                let card_class = if is_selected { "ws-preset-card active" } else { "ws-preset-card" };

                                                rsx! {
                                                    div {
                                                        class: "{card_class}",
                                                        key: "{p_id}-{pi}",
                                                        onclick: {
                                                            let draft = draft.clone();
                                                            let msg = p_msg.clone();
                                                            move |_| {
                                                                selected_preset_id.set(Some(p_id));
                                                                save_feedback.set(false);
                                                                let mut d = draft.clone();
                                                                d.ws_message = msg.clone();
                                                                on_change.call(d);
                                                            }
                                                        },
                                                        div { class: "ws-preset-info",
                                                            if is_renaming {
                                                                input {
                                                                    class: "ws-preset-rename-input",
                                                                    value: "{rename_draft()}",
                                                                    autofocus: true,
                                                                    onclick: move |e| e.stop_propagation(),
                                                                    onmousedown: move |e| e.stop_propagation(),
                                                                    oninput: move |e: Event<FormData>| rename_draft.set(e.value()),
                                                                    onkeydown: {
                                                                        let draft = draft.clone();
                                                                        move |e: KeyboardEvent| {
                                                                            e.stop_propagation();
                                                                            match e.key() {
                                                                                Key::Enter => {
                                                                                    let next = rename_draft().trim().to_string();
                                                                                    if !next.is_empty() {
                                                                                        let mut d = draft.clone();
                                                                                        if let Some(p) = d.ws_presets.iter_mut().find(|p| p.id == p_id) {
                                                                                            p.name = next;
                                                                                            on_change.call(d);
                                                                                        }
                                                                                    }
                                                                                    renaming_preset_id.set(None);
                                                                                    rename_draft.set(String::new());
                                                                                }
                                                                                Key::Escape => {
                                                                                    renaming_preset_id.set(None);
                                                                                    rename_draft.set(String::new());
                                                                                }
                                                                                _ => {}
                                                                            }
                                                                        }
                                                                    },
                                                                    onblur: {
                                                                        let draft = draft.clone();
                                                                        move |_| {
                                                                            let next = rename_draft().trim().to_string();
                                                                            if !next.is_empty() {
                                                                                let mut d = draft.clone();
                                                                                if let Some(p) = d.ws_presets.iter_mut().find(|p| p.id == p_id) {
                                                                                    p.name = next;
                                                                                    on_change.call(d);
                                                                                }
                                                                            }
                                                                            renaming_preset_id.set(None);
                                                                            rename_draft.set(String::new());
                                                                        }
                                                                    }
                                                                }
                                                            } else {
                                                                div { class: "ws-preset-name-row",
                                                                    span {
                                                                        class: "ws-preset-name",
                                                                        title: "Double click to rename preset",
                                                                        ondoubleclick: {
                                                                            let name = p_name.clone();
                                                                            move |e| {
                                                                                e.stop_propagation();
                                                                                rename_draft.set(name.clone());
                                                                                renaming_preset_id.set(Some(p_id));
                                                                            }
                                                                        },
                                                                        "{p_name}"
                                                                    }
                                                                    button {
                                                                        class: "btn-icon-rename",
                                                                        r#type: "button",
                                                                        title: "Rename preset",
                                                                        onclick: {
                                                                            let name = p_name.clone();
                                                                            move |e| {
                                                                                e.stop_propagation();
                                                                                rename_draft.set(name.clone());
                                                                                renaming_preset_id.set(Some(p_id));
                                                                            }
                                                                        },
                                                                        "✎"
                                                                    }
                                                                }
                                                                span { class: "ws-preset-snippet", "{p_msg}" }
                                                            }
                                                        }
                                                        div { class: "ws-preset-actions",
                                                            button {
                                                                class: if is_selected { "btn btn-sm btn-icon active" } else { "btn btn-sm btn-icon" },
                                                                title: "Edit preset frame in composer",
                                                                onclick: {
                                                                    let draft = draft.clone();
                                                                    let msg = p_msg.clone();
                                                                    move |e| {
                                                                        e.stop_propagation();
                                                                        selected_preset_id.set(Some(p_id));
                                                                        save_feedback.set(false);
                                                                        let mut d = draft.clone();
                                                                        d.ws_message = msg.clone();
                                                                        on_change.call(d);
                                                                    }
                                                                },
                                                                "✎"
                                                            }
                                                            button {
                                                                class: "btn btn-sm btn-icon text-muted",
                                                                title: "Delete preset",
                                                                onclick: {
                                                                    let draft = draft.clone();
                                                                    move |e| {
                                                                        e.stop_propagation();
                                                                        if selected_preset_id() == Some(p_id) {
                                                                            selected_preset_id.set(None);
                                                                        }
                                                                        if renaming_preset_id() == Some(p_id) {
                                                                            renaming_preset_id.set(None);
                                                                        }
                                                                        let mut d = draft.clone();
                                                                        d.ws_presets.retain(|p| p.id != p_id);
                                                                        on_change.call(d);
                                                                    }
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

                                div {
                                    class: "ws-deck-main",
                                    onmousemove: move |evt| {
                                        if let Some((start_y, start_h)) = composer_drag() {
                                            let dy = start_y - evt.client_coordinates().y;
                                            let next_h = (start_h + dy as i32).clamp(70, 600);
                                            composer_h.set(next_h);
                                        }
                                    },
                                    onmouseup: move |_| {
                                        if composer_drag().is_some() {
                                            composer_drag.set(None);
                                        }
                                    },
                                    div { class: "ws-stream-head",
                                        span { class: "ws-deck-title", "Live Timeline Stream ({ws_timeline.len()} frames)" }
                                    }

                                    div { class: "ws-stream-timeline",
                                        if ws_timeline.is_empty() {
                                            div { class: "ws-stream-empty",
                                                div { class: "ws-stream-empty-icon", "⚡" }
                                                div { class: "ws-stream-empty-text", "WebSocket stream is empty. Connect and send messages or presets to see live bidirectional traffic." }
                                            }
                                        }
                                        for (fi, frame) in ws_timeline.iter().enumerate() {
                                            {
                                                let f_dir = frame.direction;
                                                let f_text = frame.text.clone();
                                                let f_time = frame.timestamp.format("%H:%M:%S%.3f").to_string();
                                                let dir_class = match f_dir {
                                                    crate::core::WsDirection::Inbound => "frame-inbound",
                                                    crate::core::WsDirection::Outbound => "frame-outbound",
                                                    crate::core::WsDirection::System => "frame-system",
                                                };
                                                let badge_label = match f_dir {
                                                    crate::core::WsDirection::Inbound => "↓ IN",
                                                    crate::core::WsDirection::Outbound => "↑ OUT",
                                                    crate::core::WsDirection::System => "ℹ SYS",
                                                };
                                                rsx! {
                                                    div { class: "ws-frame-row {dir_class}", key: "{frame.id}-{fi}",
                                                        div { class: "ws-frame-bubble",
                                                            div { class: "ws-frame-meta",
                                                                span { class: "ws-frame-badge", "{badge_label}" }
                                                                span { class: "ws-frame-time", "{f_time}" }
                                                                if f_dir != crate::core::WsDirection::System {
                                                                    button {
                                                                        class: "ws-frame-copy",
                                                                        title: "Load into composer",
                                                                        onclick: {
                                                                            let draft = draft.clone();
                                                                            let t = f_text.clone();
                                                                            move |_| {
                                                                                let mut d = draft.clone();
                                                                                d.ws_message = t.clone();
                                                                                on_change.call(d);
                                                                            }
                                                                        },
                                                                        "Use"
                                                                    }
                                                                }
                                                            }
                                                            pre { class: "ws-frame-content", "{f_text}" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    div {
                                        class: if composer_drag().is_some() { "ws-composer-resizer active" } else { "ws-composer-resizer" },
                                        title: "Drag up/down to adjust input height",
                                        onmousedown: move |evt| {
                                            evt.prevent_default();
                                            evt.stop_propagation();
                                            composer_drag.set(Some((evt.client_coordinates().y, composer_h())));
                                        },
                                    }

                                    div { class: "ws-deck-composer",
                                        div { class: "ws-composer-main",
                                            div { class: "ws-composer-toolbar",
                                                div { class: "body-editor-tools-left",
                                                    button {
                                                        class: "btn btn-sm btn-ghost",
                                                        r#type: "button",
                                                        title: "Format and indent JSON message",
                                                        onclick: {
                                                            let draft = draft.clone();
                                                            move |_| {
                                                                if let Some(pretty) = crate::core::beautify_json_body(&draft.ws_message) {
                                                                    let mut d = draft.clone();
                                                                    d.ws_message = pretty;
                                                                    on_change.call(d);
                                                                }
                                                            }
                                                        },
                                                        super::icons::ActionIcon { name: "format", size: 15 }
                                                        span { "Beautify JSON" }
                                                    }
                                                    div { class: "ws-height-presets",
                                                        button {
                                                            class: if composer_h() <= 90 { "btn active" } else { "btn" },
                                                            r#type: "button",
                                                            title: "Compact height (80px)",
                                                            onclick: move |_| composer_h.set(80),
                                                            "S"
                                                        }
                                                        button {
                                                            class: if composer_h() > 90 && composer_h() <= 200 { "btn active" } else { "btn" },
                                                            r#type: "button",
                                                            title: "Medium height (160px)",
                                                            onclick: move |_| composer_h.set(160),
                                                            "M"
                                                        }
                                                        button {
                                                            class: if composer_h() > 200 { "btn active" } else { "btn" },
                                                            r#type: "button",
                                                            title: "Large height (320px)",
                                                            onclick: move |_| composer_h.set(320),
                                                            "L"
                                                        }
                                                    }
                                                }
                                                div { class: "body-editor-tools-right",
                                                    match crate::core::lint_json_body(&draft.ws_message) {
                                                        Ok(()) if !draft.ws_message.trim().is_empty() => rsx! {
                                                            span { class: "json-lint-badge json-lint-ok", "✓ Valid JSON" }
                                                        },
                                                        Err((line, col, msg)) => rsx! {
                                                            span {
                                                                class: "json-lint-badge json-lint-err",
                                                                title: "Line {line}, Column {col}: {msg}",
                                                                "⚠ Ln {line}, Col {col}: {msg}"
                                                            }
                                                        },
                                                        _ => rsx! {}
                                                    }
                                                }
                                            }
                                            MultilineCodeField {
                                                class: "ws-composer-input".to_string(),
                                                value: draft.ws_message.clone(),
                                                placeholder: "Type JSON / text message... (Press Send Frame, ⌘↵, or a Preset)".to_string(),
                                                rows: Some("6"),
                                                style: Some(format!("height: {}px;", composer_h())),
                                                vars: vars.clone(),
                                                syntax: EditorSyntax::Json,
                                                on_change: {
                                                    let draft = draft.clone();
                                                    move |text: String| {
                                                        save_feedback.set(false);
                                                        let mut d = draft.clone();
                                                        d.ws_message = text;
                                                        on_change.call(d);
                                                    }
                                                },
                                                on_submit: {
                                                    let on_protocol_action = on_protocol_action;
                                                    let ws_connected = ws_connected;
                                                    Some(Callback::new(move |_| {
                                                        if ws_connected {
                                                            on_protocol_action.call("ws_send".into());
                                                        }
                                                    }))
                                                },
                                            }
                                        }
                                        div { class: "ws-composer-actions",
                                            button {
                                                class: "btn btn-send",
                                                disabled: !ws_connected || draft.ws_message.trim().is_empty(),
                                                onclick: move |_| on_protocol_action.call("ws_send".into()),
                                                "Send Frame"
                                            }
                                            if let Some(ref sel) = selected_preset {
                                                button {
                                                    class: "btn btn-ghost",
                                                    disabled: draft.ws_message.trim().is_empty(),
                                                    title: "Save frame changes to preset \"{sel.name}\"",
                                                    onclick: {
                                                        let draft = draft.clone();
                                                        let sel_id = sel.id;
                                                        move |_| {
                                                            let mut d = draft.clone();
                                                            if let Some(p) = d.ws_presets.iter_mut().find(|p| p.id == sel_id) {
                                                                p.message = d.ws_message.clone();
                                                                on_change.call(d);
                                                                save_feedback.set(true);
                                                            }
                                                        }
                                                    },
                                                    if save_feedback() { "Saved ✓" } else { "Save Preset" }
                                                }
                                            } else {
                                                button {
                                                    class: "btn btn-ghost",
                                                    disabled: draft.ws_message.trim().is_empty(),
                                                    title: "Save current frame as new preset",
                                                    onclick: {
                                                        let draft = draft.clone();
                                                        move |_| {
                                                            let mut d = draft.clone();
                                                            let name = format!("Preset #{}", d.ws_presets.len() + 1);
                                                            let new_preset = crate::core::WsPreset::new(name, &d.ws_message);
                                                            let new_id = new_preset.id;
                                                            d.ws_presets.push(new_preset);
                                                            selected_preset_id.set(Some(new_id));
                                                            on_change.call(d);
                                                            save_feedback.set(true);
                                                        }
                                                    },
                                                    if save_feedback() { "Saved ✓" } else { "Save Preset" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                RequestProtocol::Grpc => rsx! {
                    label { class: "hint", "Method (package.Service/Method)" }
                    input {
                        class: "url-input",
                        value: "{draft.grpc_method}",
                        oninput: {
                            let draft = draft.clone();
                            move |evt: Event<FormData>| {
                                let mut d = draft.clone();
                                d.grpc_method = evt.value();
                                on_change.call(d);
                            }
                        }
                    }
                    div { class: "body-editor-toolbar",
                        div { class: "body-editor-tools-left",
                            button {
                                class: "btn btn-sm btn-ghost",
                                r#type: "button",
                                title: "Format and indent JSON body",
                                onclick: {
                                    let draft = draft.clone();
                                    move |_| {
                                        if let Some(pretty) = crate::core::beautify_json_body(&draft.body) {
                                            let mut d = draft.clone();
                                            d.body = pretty;
                                            on_change.call(d);
                                        }
                                    }
                                },
                                super::icons::ActionIcon { name: "format", size: 15 }
                                span { "Beautify JSON" }
                            }
                        }
                        div { class: "body-editor-tools-right",
                            match crate::core::lint_json_body(&draft.body) {
                                Ok(()) if !draft.body.trim().is_empty() => rsx! {
                                    span { class: "json-lint-badge json-lint-ok", "✓ Valid JSON" }
                                },
                                Err((line, col, msg)) => rsx! {
                                    span {
                                        class: "json-lint-badge json-lint-err",
                                        title: "Line {line}, Column {col}: {msg}",
                                        "⚠ Ln {line}, Col {col}: {msg}"
                                    }
                                },
                                _ => rsx! {}
                            }
                        }
                    }
                    MultilineCodeField {
                        class: "body-editor".to_string(),
                        value: draft.body.clone(),
                        placeholder: "{\"message\": \"hello\"}".to_string(),
                        rows: Some("6"),
                        vars: vars.clone(),
                        syntax: EditorSyntax::Json,
                        on_change: {
                            let draft = draft.clone();
                            move |text: String| {
                                let mut d = draft.clone();
                                d.body = text;
                                on_change.call(d);
                            }
                        }
                    }
                },
                RequestProtocol::Mqtt => rsx! {
                    label { class: "hint", "Client ID" }
                    input {
                        class: "url-input",
                        value: "{draft.mqtt_client_id}",
                        oninput: {
                            let draft = draft.clone();
                            move |evt: Event<FormData>| {
                                let mut d = draft.clone();
                                d.mqtt_client_id = evt.value();
                                on_change.call(d);
                            }
                        }
                    }
                    label { class: "hint", "Topic" }
                    input {
                        class: "url-input",
                        value: "{draft.mqtt_topic}",
                        oninput: {
                            let draft = draft.clone();
                            move |evt: Event<FormData>| {
                                let mut d = draft.clone();
                                d.mqtt_topic = evt.value();
                                on_change.call(d);
                            }
                        }
                    }
                    label { class: "hint", "Payload" }
                    MultilineCodeField {
                        class: "body-editor".to_string(),
                        value: draft.body.clone(),
                        placeholder: "Payload text or JSON".to_string(),
                        rows: Some("3"),
                        vars: vars.clone(),
                        syntax: EditorSyntax::Plain,
                        on_change: {
                            let draft = draft.clone();
                            move |text: String| {
                                let mut d = draft.clone();
                                d.body = text;
                                on_change.call(d);
                            }
                        }
                    }
                    div { class: "kv-actions",
                        button {
                            class: "btn btn-send",
                            disabled: sending,
                            onclick: move |_| on_protocol_action.call("mqtt_publish".into()),
                            "Publish"
                        }
                        button {
                            class: "btn btn-ghost",
                            disabled: sending,
                            onclick: move |_| on_protocol_action.call("mqtt_subscribe".into()),
                            "Subscribe once"
                        }
                    }
                },
                RequestProtocol::Mcp => rsx! {
                    label { class: "hint", "STDIO command (e.g. npx)" }
                    input {
                        class: "url-input",
                        value: "{draft.mcp_command}",
                        placeholder: "npx",
                        oninput: {
                            let draft = draft.clone();
                            move |evt: Event<FormData>| {
                                let mut d = draft.clone();
                                d.mcp_command = evt.value();
                                on_change.call(d);
                            }
                        }
                    }
                    label { class: "hint", "Args" }
                    input {
                        class: "url-input",
                        value: "{draft.mcp_args}",
                        placeholder: "-y @modelcontextprotocol/server-everything",
                        oninput: {
                            let draft = draft.clone();
                            move |evt: Event<FormData>| {
                                let mut d = draft.clone();
                                d.mcp_args = evt.value();
                                on_change.call(d);
                            }
                        }
                    }
                    label { class: "hint", "Tool name" }
                    input {
                        class: "url-input",
                        value: "{draft.mcp_tool_name}",
                        oninput: {
                            let draft = draft.clone();
                            move |evt: Event<FormData>| {
                                let mut d = draft.clone();
                                d.mcp_tool_name = evt.value();
                                on_change.call(d);
                            }
                        }
                    }
                    label { class: "hint", "Tool args JSON" }
                    div { class: "body-editor-toolbar",
                        div { class: "body-editor-tools-left",
                            button {
                                class: "btn btn-sm btn-ghost",
                                r#type: "button",
                                title: "Format and indent JSON args",
                                onclick: {
                                    let draft = draft.clone();
                                    move |_| {
                                        if let Some(pretty) = crate::core::beautify_json_body(&draft.mcp_tool_args_json) {
                                            let mut d = draft.clone();
                                            d.mcp_tool_args_json = pretty;
                                            on_change.call(d);
                                        }
                                    }
                                },
                                super::icons::ActionIcon { name: "format", size: 15 }
                                span { "Beautify JSON" }
                            }
                        }
                        div { class: "body-editor-tools-right",
                            match crate::core::lint_json_body(&draft.mcp_tool_args_json) {
                                Ok(()) if !draft.mcp_tool_args_json.trim().is_empty() => rsx! {
                                    span { class: "json-lint-badge json-lint-ok", "✓ Valid JSON" }
                                },
                                Err((line, col, msg)) => rsx! {
                                    span {
                                        class: "json-lint-badge json-lint-err",
                                        title: "Line {line}, Column {col}: {msg}",
                                        "⚠ Ln {line}, Col {col}: {msg}"
                                    }
                                },
                                _ => rsx! {}
                            }
                        }
                    }
                    MultilineCodeField {
                        class: "body-editor".to_string(),
                        value: draft.mcp_tool_args_json.clone(),
                        placeholder: "{\"param\": \"value\"}".to_string(),
                        rows: Some("4"),
                        vars: vars.clone(),
                        syntax: EditorSyntax::Json,
                        on_change: {
                            let draft = draft.clone();
                            move |text: String| {
                                let mut d = draft.clone();
                                d.mcp_tool_args_json = text;
                                on_change.call(d);
                            }
                        }
                    }
                    div { class: "kv-actions",
                        button {
                            class: "btn btn-send",
                            disabled: sending,
                            onclick: move |_| on_protocol_action.call("mcp_list".into()),
                            "List tools"
                        }
                        button {
                            class: "btn btn-ghost",
                            disabled: sending,
                            onclick: move |_| on_protocol_action.call("mcp_call".into()),
                            "Call tool"
                        }
                    }
                },
                RequestProtocol::SocketIo => rsx! {
                    label { class: "hint", "Event name (emit / listen)" }
                    input {
                        class: "url-input",
                        value: "{draft.socketio_event}",
                        oninput: {
                            let draft = draft.clone();
                            move |evt: Event<FormData>| {
                                let mut d = draft.clone();
                                d.socketio_event = evt.value();
                                on_change.call(d);
                            }
                        }
                    }
                    label { class: "hint", "Emit payload JSON" }
                    div { class: "body-editor-toolbar",
                        div { class: "body-editor-tools-left",
                            button {
                                class: "btn btn-sm btn-ghost",
                                r#type: "button",
                                title: "Format and indent JSON payload",
                                onclick: {
                                    let draft = draft.clone();
                                    move |_| {
                                        if let Some(pretty) = crate::core::beautify_json_body(&draft.body) {
                                            let mut d = draft.clone();
                                            d.body = pretty;
                                            on_change.call(d);
                                        }
                                    }
                                },
                                super::icons::ActionIcon { name: "format", size: 15 }
                                span { "Beautify JSON" }
                            }
                        }
                        div { class: "body-editor-tools-right",
                            match crate::core::lint_json_body(&draft.body) {
                                Ok(()) if !draft.body.trim().is_empty() => rsx! {
                                    span { class: "json-lint-badge json-lint-ok", "✓ Valid JSON" }
                                },
                                Err((line, col, msg)) => rsx! {
                                    span {
                                        class: "json-lint-badge json-lint-err",
                                        title: "Line {line}, Column {col}: {msg}",
                                        "⚠ Ln {line}, Col {col}: {msg}"
                                    }
                                },
                                _ => rsx! {}
                            }
                        }
                    }
                    MultilineCodeField {
                        class: "body-editor".to_string(),
                        value: draft.body.clone(),
                        placeholder: "{\"event\": \"data\"}".to_string(),
                        rows: Some("4"),
                        vars: vars.clone(),
                        syntax: EditorSyntax::Json,
                        on_change: {
                            let draft = draft.clone();
                            move |text: String| {
                                let mut d = draft.clone();
                                d.body = text;
                                on_change.call(d);
                            }
                        }
                    }
                    div { class: "kv-actions",
                        button {
                            class: "btn btn-send",
                            disabled: sending,
                            onclick: move |_| on_protocol_action.call("socketio_session".into()),
                            "Connect / emit / listen"
                        }
                    }
                },
                RequestProtocol::Sse => rsx! {
                    SseWorkbench {
                        draft: draft.clone(),
                        vars: vars.clone(),
                        sending,
                        protocol_log: protocol_log.clone(),
                        on_change,
                        on_protocol_action,
                    }
                },
                RequestProtocol::GraphqlWs => rsx! {
                    label { class: "hint", "Subscription query" }
                    CommentableTextarea {
                        class: "body-editor".to_string(),
                        value: draft.graphql_ws_query.clone(),
                        rows: Some("6"),
                        placeholder: "subscription {{ … }}".to_string(),
                        vars: vars.clone(),
                        on_change: {
                            let draft = draft.clone();
                            move |text: String| {
                                let mut d = draft.clone();
                                d.graphql_ws_query = text;
                                on_change.call(d);
                            }
                        }
                    }
                    label { class: "hint", "Variables JSON" }
                    div { class: "body-editor-toolbar",
                        div { class: "body-editor-tools-left",
                            button {
                                class: "btn btn-sm btn-ghost",
                                r#type: "button",
                                title: "Format and indent JSON variables",
                                onclick: {
                                    let draft = draft.clone();
                                    move |_| {
                                        if let Some(pretty) = crate::core::beautify_json_body(&draft.graphql_variables) {
                                            let mut d = draft.clone();
                                            d.graphql_variables = pretty;
                                            on_change.call(d);
                                        }
                                    }
                                },
                                super::icons::ActionIcon { name: "format", size: 15 }
                                span { "Beautify JSON" }
                            }
                        }
                        div { class: "body-editor-tools-right",
                            match crate::core::lint_json_body(&draft.graphql_variables) {
                                Ok(()) if !draft.graphql_variables.trim().is_empty() => rsx! {
                                    span { class: "json-lint-badge json-lint-ok", "✓ Valid JSON" }
                                },
                                Err((line, col, msg)) => rsx! {
                                    span {
                                        class: "json-lint-badge json-lint-err",
                                        title: "Line {line}, Column {col}: {msg}",
                                        "⚠ Ln {line}, Col {col}: {msg}"
                                    }
                                },
                                _ => rsx! {}
                            }
                        }
                    }
                    MultilineCodeField {
                        class: "body-editor".to_string(),
                        value: draft.graphql_variables.clone(),
                        placeholder: "{\"var\": \"val\"}".to_string(),
                        rows: Some("3"),
                        vars: vars.clone(),
                        syntax: EditorSyntax::Json,
                        on_change: {
                            let draft = draft.clone();
                            move |text: String| {
                                let mut d = draft.clone();
                                d.graphql_variables = text;
                                on_change.call(d);
                            }
                        }
                    }
                    div { class: "kv-actions",
                        button {
                            class: "btn btn-send",
                            disabled: sending,
                            onclick: move |_| on_protocol_action.call("graphql_ws_subscribe".into()),
                            "Subscribe"
                        }
                    }
                },
                RequestProtocol::Http => rsx! {},
            }
            if !protocol_log.is_empty()
                && draft.protocol != RequestProtocol::Sse
                && (draft.protocol != RequestProtocol::WebSocket || !draft.ws_deck_mode)
            {
                div { class: "protocol-log",
                    h4 { "Protocol log" }
                    for (idx, line) in protocol_log.iter().enumerate() {
                        pre { key: "plog-{idx}", class: "console-line", "{line}" }
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct SseParsedMeta {
    state: String,
    status: u16,
    status_text: String,
    connect_ms: Option<u64>,
    elapsed_ms: u64,
    ttfb_ms: Option<u64>,
    count: usize,
    warning: Option<String>,
    method: String,
    url: String,
}

#[derive(Clone, Debug, PartialEq)]
struct SseParsedEvent {
    index: usize,
    event: String,
    id: String,
    elapsed_ms: u64,
    data: String,
}

fn parse_sse_log(protocol_log: &[String]) -> (Option<SseParsedMeta>, Vec<SseParsedEvent>, Option<String>) {
    let mut meta = None;
    let mut events = Vec::new();
    let mut err_msg = None;

    for line in protocol_log {
        if let Some(raw) = line.strip_prefix("SSE_META:") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
                let status = v.get("status").and_then(|x| x.as_u64()).unwrap_or(200) as u16;
                let default_state = if status == 0 { "connecting" } else { "completed" };
                meta = Some(SseParsedMeta {
                    state: v
                        .get("state")
                        .and_then(|x| x.as_str())
                        .unwrap_or(default_state)
                        .to_string(),
                    status,
                    status_text: v.get("status_text").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    connect_ms: v.get("connect_ms").and_then(|x| x.as_u64()),
                    elapsed_ms: v.get("elapsed_ms").and_then(|x| x.as_u64()).unwrap_or(0),
                    ttfb_ms: v.get("ttfb_ms").and_then(|x| x.as_u64()),
                    count: v.get("count").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
                    warning: v.get("warning").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    method: v.get("method").and_then(|x| x.as_str()).unwrap_or("GET").to_string(),
                    url: v.get("url").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                });
            }
        } else if let Some(raw) = line.strip_prefix("SSE_EVENT:") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
                events.push(SseParsedEvent {
                    index: v.get("index").and_then(|x| x.as_u64()).unwrap_or((events.len() + 1) as u64) as usize,
                    event: v.get("event").and_then(|x| x.as_str()).unwrap_or("message").to_string(),
                    id: v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    elapsed_ms: v.get("elapsed_ms").and_then(|x| x.as_u64()).unwrap_or(0),
                    data: v.get("data").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                });
            }
        } else if let Some(raw) = line.strip_prefix("SSE_ERR:") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
                err_msg = v.get("error").and_then(|x| x.as_str()).map(|s| s.to_string());
            } else {
                err_msg = Some(raw.to_string());
            }
        } else if err_msg.is_none() && !line.trim().is_empty() && meta.is_none() {
            err_msg = Some(line.clone());
        }
    }

    (meta, events, err_msg)
}

fn extract_ai_delta_text(data: &str) -> Option<String> {
    let trimmed = data.trim();
    if trimmed.is_empty() || trimmed == "[DONE]" {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    for ptr in [
        "/choices/0/delta/content",
        "/choices/0/delta/reasoning_content",
        "/choices/0/text",
        "/delta/text",
        "/message/content",
        "/response",
        "/content/0/text",
        "/token",
        "/text",
    ] {
        if let Some(s) = v.pointer(ptr).and_then(|x| x.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn format_sse_data_display(data: &str) -> (String, bool) {
    let t = data.trim();
    if (t.starts_with('{') || t.starts_with('[')) && t != "[DONE]" {
        if let Some(pretty) = crate::core::beautify_json_body(t) {
            return (pretty, true);
        }
    }
    (data.to_string(), false)
}

#[component]
fn SseWorkbench(
    draft: RequestDraft,
    vars: Option<VariableContext>,
    sending: bool,
    protocol_log: Vec<String>,
    on_change: EventHandler<RequestDraft>,
    on_protocol_action: EventHandler<String>,
) -> Element {
    let mut max_events = use_signal(|| 40usize);
    let mut timeout_secs = use_signal(|| 20u64);
    let mut event_filter = use_signal(String::new);
    let mut raw_view = use_signal(|| false);
    let mut copied_assembled = use_signal(|| false);

    let (meta_opt, events, err_opt) = parse_sse_log(&protocol_log);
    let has_done = events.iter().any(|e| e.data.trim() == "[DONE]");

    let assembled_ai_text: String = events
        .iter()
        .filter_map(|e| extract_ai_delta_text(&e.data))
        .collect();

    let filter_lc = event_filter().trim().to_ascii_lowercase();
    let filtered_events: Vec<&SseParsedEvent> = events
        .iter()
        .filter(|e| {
            filter_lc.is_empty()
                || e.event.to_ascii_lowercase().contains(&filter_lc)
                || e.data.to_ascii_lowercase().contains(&filter_lc)
                || e.id.to_ascii_lowercase().contains(&filter_lc)
        })
        .collect();

    let show_body_editor = draft.method != HttpMethod::Get && draft.method != HttpMethod::Head;
    let vctx = vars.unwrap_or_default();

    rsx! {
        div { class: "sse-workbench",
            div { class: "sse-control-bar",
                div { class: "sse-control-left",
                    span { class: "sse-proto-badge", "SSE STREAM" }
                    span { class: "sse-method-pill", "{draft.method}" }
                    span { class: "sse-hint-inline",
                        if show_body_editor {
                            "Streaming HTTP {draft.method} (AI / API JSON body + EventSource frames)"
                        } else {
                            "EventSource GET stream — switch method above to POST for AI / LLM streaming APIs"
                        }
                    }
                }
                div { class: "sse-control-right",
                    label { class: "sse-inline-field",
                        span { "Max events" }
                        select {
                            class: "sse-compact-select",
                            value: "{max_events()}",
                            onchange: move |evt| {
                                if let Ok(v) = evt.value().parse::<usize>() {
                                    max_events.set(v);
                                }
                            },
                            option { value: "20", "20" }
                            option { value: "40", "40" }
                            option { value: "100", "100" }
                            option { value: "250", "250" }
                        }
                    }
                    label { class: "sse-inline-field",
                        span { "Timeout" }
                        select {
                            class: "sse-compact-select",
                            value: "{timeout_secs()}",
                            onchange: move |evt| {
                                if let Ok(v) = evt.value().parse::<u64>() {
                                    timeout_secs.set(v);
                                }
                            },
                            option { value: "15", "15s" }
                            option { value: "20", "20s" }
                            option { value: "45", "45s" }
                            option { value: "90", "90s" }
                        }
                    }
                    button {
                        class: if sending { "btn btn-ghost btn-cancel sse-stream-btn" } else { "btn btn-send sse-stream-btn" },
                        r#type: "button",
                        onclick: move |_| {
                            if sending {
                                on_protocol_action.call("sse_stop".into());
                            } else {
                                on_protocol_action.call(format!(
                                    "sse_listen:{}:{}",
                                    max_events(),
                                    timeout_secs() * 1000
                                ));
                            }
                        },
                        if sending { "Disconnect" } else { "Connect & Stream" }
                    }
                }
            }

            if show_body_editor {
                div { class: "sse-body-card",
                    div { class: "body-editor-toolbar",
                        div { class: "body-editor-tools-left",
                            span { class: "sse-section-title", "JSON Stream Request Payload" }
                            button {
                                class: "btn btn-sm btn-ghost",
                                r#type: "button",
                                title: "Insert OpenAI-compatible AI streaming request body",
                                onclick: {
                                    let draft = draft.clone();
                                    move |_| {
                                        let mut d = draft.clone();
                                        d.body_mode = BodyMode::Raw;
                                        d.body = "{\n  \"model\": \"gpt-4o-mini\",\n  \"stream\": true,\n  \"messages\": [\n    { \"role\": \"user\", \"content\": \"Hello!\" }\n  ]\n}".to_string();
                                        on_change.call(d);
                                    }
                                },
                                "✨ AI Stream Template"
                            }
                            button {
                                class: "btn btn-sm btn-ghost",
                                r#type: "button",
                                title: "Format and indent JSON body",
                                onclick: {
                                    let draft = draft.clone();
                                    move |_| {
                                        if let Some(pretty) = crate::core::beautify_json_body(&draft.body) {
                                            let mut d = draft.clone();
                                            d.body_mode = BodyMode::Raw;
                                            d.body = pretty;
                                            on_change.call(d);
                                        }
                                    }
                                },
                                "Beautify JSON"
                            }
                        }
                        div { class: "body-editor-tools-right",
                            match crate::core::lint_json_body(&draft.body) {
                                Ok(()) if !draft.body.trim().is_empty() => rsx! {
                                    span { class: "json-lint-badge json-lint-ok", "✓ Valid JSON" }
                                },
                                Err((line, col, msg)) => rsx! {
                                    span {
                                        class: "json-lint-badge json-lint-err",
                                        title: "Line {line}, Column {col}: {msg}",
                                        "⚠ Ln {line}, Col {col}: {msg}"
                                    }
                                },
                                _ => rsx! {}
                            }
                        }
                    }
                    MultilineCodeField {
                        key: "sse-body-{draft.id}",
                        class: "body-editor sse-body-editor".to_string(),
                        value: draft.body.clone(),
                        placeholder: "{\"stream\": true, \"messages\": [...]}".to_string(),
                        rows: Some("5"),
                        vars: Some(vctx),
                        syntax: EditorSyntax::Json,
                        on_change: {
                            let draft = draft.clone();
                            move |text: String| {
                                let mut d = draft.clone();
                                d.body_mode = BodyMode::Raw;
                                d.body = text;
                                on_change.call(d);
                            }
                        }
                    }
                }
            }

            if let Some(ref err) = err_opt {
                div { class: "sse-error-card",
                    div { class: "sse-error-head", "⚠ SSE Stream Error" }
                    pre { class: "sse-error-body", "{err}" }
                    p { class: "sse-error-tip",
                        "Tip: Check HTTP method (GET vs POST for AI APIs), Authorization header, and Headers tab defaults (Accept: text/event-stream)."
                    }
                }
            }

            if let Some(ref meta) = meta_opt {
                div { class: "sse-telemetry-strip",
                    if meta.state == "connecting" || meta.status == 0 {
                        span { class: "sse-chip", "◌ Connecting…" }
                        if !meta.url.is_empty() {
                            span { class: "sse-chip", "{meta.method} {meta.url}" }
                        }
                    } else {
                        span { class: "sse-chip sse-chip-ok",
                            if sending {
                                "● Connected · HTTP {meta.status} {meta.status_text}"
                            } else {
                                "✓ Connected · HTTP {meta.status} {meta.status_text}"
                            }
                        }
                        if let Some(cms) = meta.connect_ms {
                            span { class: "sse-chip", "Handshake {cms} ms" }
                        }
                        span { class: "sse-chip", "{meta.count} events" }
                        if let Some(ttfb) = meta.ttfb_ms {
                            span { class: "sse-chip", "First event {ttfb} ms" }
                        }
                        if !sending {
                            span { class: "sse-chip", "Total {meta.elapsed_ms} ms" }
                        }
                        if has_done {
                            span { class: "sse-chip sse-chip-done", "✓ [DONE]" }
                        }
                        if let Some(ref w) = meta.warning {
                            span { class: "sse-chip sse-chip-warn", "⚠ {w}" }
                        }
                    }
                }
                if err_opt.is_none() && events.is_empty() {
                    if meta.state == "connecting" || meta.status == 0 {
                        div { class: "sse-connected-card connecting",
                            div { class: "sse-connected-head", "◌ Connecting to SSE Endpoint…" }
                            p { class: "sse-connected-sub",
                                "Waiting for HTTP response headers from {meta.url}…"
                            }
                        }
                    } else {
                        div { class: "sse-connected-card",
                            div { class: "sse-connected-head",
                                if sending {
                                    "✓ Connection Successful (HTTP {meta.status} {meta.status_text}) — Listening for Events…"
                                } else {
                                    "✓ Connection Successful (HTTP {meta.status} {meta.status_text})"
                                }
                            }
                            p { class: "sse-connected-sub",
                                if sending {
                                    "Connected to {meta.url}. Stream is open and incoming Server-Sent Events will appear live below."
                                } else if let Some(ref w) = meta.warning {
                                    "{w}"
                                } else {
                                    "Connected to {meta.url}; stream closed cleanly with 0 events."
                                }
                            }
                        }
                    }
                }
            }

            if !assembled_ai_text.is_empty() {
                div { class: "sse-ai-preview-card",
                    div { class: "sse-ai-preview-head",
                        span { class: "sse-ai-preview-title", "✨ Assembled AI Stream Output" }
                        button {
                            class: "console-btn-text",
                            r#type: "button",
                            onclick: {
                                let txt = assembled_ai_text.clone();
                                move |_| {
                                    let script = format!(
                                        "navigator.clipboard.writeText({});",
                                        serde_json::to_string(&txt).unwrap_or_else(|_| "\"\"".into())
                                    );
                                    spawn(async move {
                                        let _ = document::eval(&script).await;
                                        tokio::time::sleep(std::time::Duration::from_millis(1600)).await;
                                        copied_assembled.set(false);
                                    });
                                    copied_assembled.set(true);
                                }
                            },
                            if copied_assembled() { "✓ Copied" } else { "Copy Text" }
                        }
                    }
                    pre { class: "sse-ai-preview-body", "{assembled_ai_text}" }
                }
            }

            if !events.is_empty() {
                div { class: "sse-events-toolbar",
                    span { class: "sse-section-title", "Events ({filtered_events.len()} / {events.len()})" }
                    div { class: "sse-events-tools-right",
                        input {
                            class: "sse-filter-input",
                            r#type: "text",
                            placeholder: "Filter event type or payload…",
                            value: "{event_filter()}",
                            oninput: move |evt| event_filter.set(evt.value()),
                        }
                        button {
                            class: if !raw_view() { "console-btn-text active" } else { "console-btn-text" },
                            r#type: "button",
                            onclick: move |_| raw_view.set(false),
                            "Cards"
                        }
                        button {
                            class: if raw_view() { "console-btn-text active" } else { "console-btn-text" },
                            r#type: "button",
                            onclick: move |_| raw_view.set(true),
                            "Raw"
                        }
                    }
                }
                if raw_view() {
                    pre { class: "sse-raw-stream",
                        for ev in &filtered_events {
                            if !ev.id.is_empty() {
                                "id: {ev.id}\n"
                            }
                            "event: {ev.event}\ndata: {ev.data}\n\n"
                        }
                    }
                } else {
                    div { class: "sse-event-list",
                        for (ev_i, ev) in filtered_events.iter().enumerate() {
                            {
                                let (formatted, is_json) = format_sse_data_display(&ev.data);
                                let payload_copy = ev.data.clone();
                                rsx! {
                                    div { key: "sse-ev-{ev.index}-{ev_i}", class: "sse-event-card",
                                        div { class: "sse-event-card-head",
                                            span { class: "sse-ev-idx", "#{ev.index}" }
                                            span { class: "sse-ev-type", "{ev.event}" }
                                            if !ev.id.is_empty() {
                                                span { class: "sse-ev-id", "id: {ev.id}" }
                                            }
                                            span { class: "sse-ev-time", "+{ev.elapsed_ms} ms" }
                                            if is_json {
                                                span { class: "sse-ev-json-tag", "JSON" }
                                            }
                                            button {
                                                class: "console-btn-text sse-copy-ev-btn",
                                                r#type: "button",
                                                title: "Copy event payload",
                                                onclick: move |_| {
                                                    let script = format!(
                                                        "navigator.clipboard.writeText({});",
                                                        serde_json::to_string(&payload_copy).unwrap_or_else(|_| "\"\"".into())
                                                    );
                                                    spawn(async move {
                                                        let _ = document::eval(&script).await;
                                                    });
                                                },
                                                "Copy"
                                            }
                                        }
                                        pre { class: "sse-event-card-data", "{formatted}" }
                                    }
                                }
                            }
                        }
                    }
                }
            } else if err_opt.is_none() && meta_opt.is_none() && !sending {
                div { class: "sse-empty-state",
                    p { class: "sse-empty-title", "Ready to stream Server-Sent Events" }
                    p { class: "sse-empty-sub",
                        "Set URL & Headers above (e.g. Authorization: Bearer {{token}}), pick GET or POST (JSON body for AI endpoints), then click Stream / Connect & Stream."
                    }
                }
            }
        }
    }
}

fn key_values_to_raw(rows: &[KeyValue]) -> String {
    let mut out = String::new();
    for r in rows {
        if r.key.is_empty() && r.value.is_empty() {
            continue;
        }
        if !r.enabled {
            out.push_str("// ");
        }
        out.push_str(&r.key);
        out.push_str(": ");
        out.push_str(&r.value);
        out.push('\n');
    }
    out
}

fn raw_to_key_values(raw: &str) -> Vec<KeyValue> {
    let mut rows = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let (enabled, text) = if let Some(stripped) = t.strip_prefix("//") {
            (false, stripped.trim_start())
        } else {
            (true, t)
        };
        let (k, v) = if let Some((k, v)) = text.split_once(':') {
            (k.trim(), v.trim())
        } else if let Some((k, v)) = text.split_once('=') {
            (k.trim(), v.trim())
        } else {
            (text.trim(), "")
        };
        let mut kv = KeyValue::new(k, v);
        kv.enabled = enabled;
        rows.push(kv);
    }
    ensure_trailing_empty(&mut rows);
    rows
}

const HEADER_NAME_SUGGESTIONS: &[&str] = &[
    "Accept",
    "Accept-Encoding",
    "Accept-Language",
    "Authorization",
    "Cache-Control",
    "Connection",
    "Content-Type",
    "Cookie",
    "If-Modified-Since",
    "If-None-Match",
    "Last-Event-ID",
    "OpenAI-Beta",
    "OpenAI-Organization",
    "Origin",
    "Pragma",
    "Referer",
    "User-Agent",
    "X-Api-Key",
    "X-Correlation-ID",
    "X-Forwarded-For",
    "X-Request-ID",
    "anthropic-version",
];

fn header_name_suggestions(query: &str) -> Vec<&'static str> {
    let q = query.trim().to_ascii_lowercase();
    let mut matches: Vec<&'static str> = HEADER_NAME_SUGGESTIONS
        .iter()
        .copied()
        .filter(|h| q.is_empty() || h.to_ascii_lowercase().contains(&q))
        .collect();
    matches.truncate(8);
    matches
}

fn header_value_suggestions(key: &str, query: &str) -> Vec<&'static str> {
    let candidates: &[&str] = match key.trim().to_ascii_lowercase().as_str() {
        "accept" => &[
            "text/event-stream",
            "application/json",
            "*/*",
            "text/plain",
            "application/xml",
        ],
        "content-type" => &[
            "application/json",
            "application/json; charset=utf-8",
            "application/x-www-form-urlencoded",
            "multipart/form-data",
            "text/plain",
            "application/grpc",
            "text/event-stream",
        ],
        "authorization" => &[
            "Bearer {{token}}",
            "Bearer {{api_key}}",
            "Basic {{credentials}}",
            "ApiKey {{api_key}}",
        ],
        "accept-encoding" => &["identity", "gzip, deflate, br", "gzip"],
        "cache-control" => &["no-cache", "no-store", "max-age=0"],
        "connection" => &["keep-alive", "Upgrade", "close"],
        "user-agent" => &[crate::core::BLITZ_USER_AGENT, "curl/8.6.0", "Mozilla/5.0"],
        "anthropic-version" => &["2023-06-01", "2023-01-01"],
        "openai-beta" => &["assistants=v2", "realtime=v1"],
        "x-api-key" => &["{{api_key}}", "{{token}}"],
        _ => &[],
    };
    let q = query.trim().to_ascii_lowercase();
    candidates
        .iter()
        .copied()
        .filter(|v| q.is_empty() || v.to_ascii_lowercase().contains(&q))
        .take(7)
        .collect()
}

fn effective_auto_headers(
    proto: RequestProtocol,
    body_mode: Option<BodyMode>,
) -> Vec<(&'static str, &'static str)> {
    let mut out = proto.auto_generated_headers().to_vec();
    if proto == RequestProtocol::Http {
        if let Some(bm) = body_mode {
            let ct = match bm {
                BodyMode::Raw | BodyMode::Graphql => Some("application/json"),
                BodyMode::Urlencoded => Some("application/x-www-form-urlencoded"),
                BodyMode::FormData => Some("multipart/form-data"),
                BodyMode::Binary => Some("application/octet-stream"),
                BodyMode::None => None,
            };
            if let Some(ct_val) = ct {
                if !out.iter().any(|(k, _)| k.eq_ignore_ascii_case("Content-Type")) {
                    out.push(("Content-Type", ct_val));
                }
            }
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HdrAcTarget {
    Key(usize),
    Value(usize),
}

#[component]
fn KvEditor(
    rows: Vec<KeyValue>,
    vars: VariableContext,
    #[props(default)] is_headers: bool,
    #[props(default)] protocol: Option<RequestProtocol>,
    #[props(default)] body_mode: Option<BodyMode>,
    on_var_edit: EventHandler<VarEditEvent>,
    on_change: EventHandler<Vec<KeyValue>>,
) -> Element {
    let mut bulk_mode = use_signal(|| false);
    let mut bulk_text = use_signal(String::new);
    let mut show_system_headers = use_signal(|| true);
    let mut ac_target = use_signal(|| None::<HdrAcTarget>);
    let mut ac_selected = use_signal(|| 0usize);

    let proto = protocol.unwrap_or(RequestProtocol::Http);
    let auto_headers = if is_headers {
        effective_auto_headers(proto, body_mode)
    } else {
        Vec::new()
    };
    let auto_count = auto_headers.len();
    let toggle_label = if show_system_headers() {
        format!("▾ Hide protocol default headers ({auto_count})")
    } else {
        format!("▸ Show protocol default headers ({auto_count})")
    };

    rsx! {
        div { class: "kv-toolbar",
            div { style: "display: flex; gap: 8px; align-items: center; flex-wrap: wrap;",
                button {
                    class: "btn btn-sm btn-ghost",
                    r#type: "button",
                    onclick: {
                        let rows = rows.clone();
                        move |_| {
                            if !bulk_mode() {
                                bulk_text.set(key_values_to_raw(&rows));
                                bulk_mode.set(true);
                            } else {
                                let next = raw_to_key_values(&bulk_text());
                                on_change.call(next);
                                bulk_mode.set(false);
                            }
                        }
                    },
                    if bulk_mode() { "Table Edit" } else { "Bulk Edit" }
                }
                if !bulk_mode() {
                    button {
                        class: "btn btn-sm btn-ghost",
                        r#type: "button",
                        onclick: {
                            let rows = rows.clone();
                            move |_| {
                                let mut next = rows.clone();
                                if next.last().map(|r| !r.key.is_empty() || !r.value.is_empty()).unwrap_or(true) {
                                    next.push(KeyValue::new("", ""));
                                } else {
                                    next.insert(next.len().saturating_sub(1), KeyValue::new("", ""));
                                }
                                on_change.call(next);
                            }
                        },
                        if is_headers { "+ Add Header" } else { "+ Add Row" }
                    }
                }
                if is_headers && auto_count > 0 {
                    button {
                        class: "system-headers-toggle",
                        r#type: "button",
                        onclick: move |_| show_system_headers.set(!show_system_headers()),
                        "{toggle_label}"
                    }
                }
            }
        }
        if bulk_mode() {
            textarea {
                class: "bulk-edit-textarea",
                rows: 8,
                placeholder: "key: value\n// disabled_key: value",
                value: "{bulk_text()}",
                oninput: move |evt: Event<FormData>| {
                    let val = evt.value();
                    bulk_text.set(val.clone());
                    let next = raw_to_key_values(&val);
                    on_change.call(next);
                }
            }
        } else {
            table { class: "kv-table",
                thead {
                    tr {
                        th { class: "kv-col-check", "" }
                        th { "Key" }
                        th { "Value" }
                        th { class: "kv-col-action", "" }
                    }
                }
                tbody {
                    if is_headers && show_system_headers() {
                        for (ai, (k, v)) in auto_headers.iter().copied().enumerate() {
                            {
                                let overridden = rows.iter().any(|r| {
                                    r.enabled && !r.key.trim().is_empty() && r.key.trim().eq_ignore_ascii_case(k)
                                });
                                rsx! {
                                    tr {
                                        key: "auto-{ai}-{k}",
                                        class: if overridden { "system-header-row overridden" } else { "system-header-row" },
                                        td { class: "kv-col-check",
                                            span {
                                                class: if overridden { "kv-auto-badge overridden" } else { "kv-auto-badge" },
                                                title: if overridden { "Overridden by custom header row below" } else { "Default header automatically applied for {proto}" },
                                                if overridden { "OVER" } else { "AUTO" }
                                            }
                                        }
                                        td {
                                            input {
                                                r#type: "text",
                                                readonly: true,
                                                value: "{k}",
                                                title: "Default header for {proto}",
                                            }
                                        }
                                        td {
                                            input {
                                                r#type: "text",
                                                readonly: true,
                                                value: "{v}",
                                                title: "Default value for {proto}",
                                            }
                                        }
                                        td { class: "kv-col-action",
                                            button {
                                                class: "kv-override-btn",
                                                r#type: "button",
                                                title: "Copy default header into editable table to customize",
                                                disabled: overridden,
                                                onclick: {
                                                    let rows = rows.clone();
                                                    move |_| {
                                                        let mut next = rows.clone();
                                                        if !next.iter().any(|r| r.key.trim().eq_ignore_ascii_case(k)) {
                                                            let pos = if next.last().map(|r| r.key.is_empty() && r.value.is_empty()).unwrap_or(false) {
                                                                next.len() - 1
                                                            } else {
                                                                next.len()
                                                            };
                                                            next.insert(pos, KeyValue::new(k, v));
                                                            ensure_trailing_empty(&mut next);
                                                            on_change.call(next);
                                                        }
                                                    }
                                                },
                                                if overridden { "Active" } else { "Override" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    for (i, row) in rows.iter().enumerate() {
                        {
                            let is_trailing_empty = i + 1 == rows.len() && row.key.is_empty() && row.value.is_empty();
                            let key_suggestions = if is_headers && ac_target() == Some(HdrAcTarget::Key(i)) {
                                header_name_suggestions(&row.key)
                            } else {
                                Vec::new()
                            };
                            let val_suggestions = if is_headers
                                && ac_target() == Some(HdrAcTarget::Value(i))
                                && !row.value.contains("{{")
                            {
                                header_value_suggestions(&row.key, &row.value)
                            } else {
                                Vec::new()
                            };
                            let cur_ac_sel = ac_selected();
                            rsx! {
                                tr {
                                    key: "kv-{i}",
                                    class: if !row.enabled { "kv-row-disabled" } else { "" },
                                    td { class: "kv-col-check",
                                        if !is_trailing_empty {
                                            input {
                                                r#type: "checkbox",
                                                checked: row.enabled,
                                                title: if row.enabled { "Disable row" } else { "Enable row" },
                                                onchange: {
                                                    let rows = rows.clone();
                                                    move |evt: Event<FormData>| {
                                                        let mut next = rows.clone();
                                                        if let Some(r) = next.get_mut(i) {
                                                            r.enabled = evt.checked();
                                                        }
                                                        on_change.call(next);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    td {
                                        div { class: "kv-cell-ac-wrap",
                                            input {
                                                r#type: "text",
                                                placeholder: if is_headers { "Header name (e.g. Authorization)" } else { "Key" },
                                                value: "{row.key}",
                                                onfocus: move |_| {
                                                    if is_headers {
                                                        ac_target.set(Some(HdrAcTarget::Key(i)));
                                                        ac_selected.set(0);
                                                    }
                                                },
                                                onblur: move |_| {
                                                    if ac_target() == Some(HdrAcTarget::Key(i)) {
                                                        ac_target.set(None);
                                                    }
                                                },
                                                onkeydown: {
                                                    let suggestions = key_suggestions.clone();
                                                    let rows = rows.clone();
                                                    move |evt: Event<KeyboardData>| {
                                                        if suggestions.is_empty() {
                                                            return;
                                                        }
                                                        match evt.key() {
                                                            Key::ArrowDown => {
                                                                evt.prevent_default();
                                                                ac_selected.set((ac_selected() + 1) % suggestions.len());
                                                            }
                                                            Key::ArrowUp => {
                                                                evt.prevent_default();
                                                                let n = suggestions.len();
                                                                ac_selected.set((ac_selected() + n - 1) % n);
                                                            }
                                                            Key::Enter | Key::Tab => {
                                                                if let Some(&picked) = suggestions.get(ac_selected()) {
                                                                    if evt.key() == Key::Enter {
                                                                        evt.prevent_default();
                                                                    }
                                                                    let mut next = rows.clone();
                                                                    if let Some(r) = next.get_mut(i) {
                                                                        r.key = picked.to_string();
                                                                    }
                                                                    ensure_trailing_empty(&mut next);
                                                                    on_change.call(next);
                                                                    ac_target.set(None);
                                                                }
                                                            }
                                                            Key::Escape => {
                                                                ac_target.set(None);
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                },
                                                oninput: {
                                                    let rows = rows.clone();
                                                    move |evt: Event<FormData>| {
                                                        let mut next = rows.clone();
                                                        if let Some(r) = next.get_mut(i) {
                                                            r.key = evt.value();
                                                        }
                                                        ensure_trailing_empty(&mut next);
                                                        on_change.call(next);
                                                        if is_headers {
                                                            ac_target.set(Some(HdrAcTarget::Key(i)));
                                                            ac_selected.set(0);
                                                        }
                                                    }
                                                }
                                            }
                                            if !key_suggestions.is_empty() {
                                                div {
                                                    class: "hdr-ac-menu",
                                                    onmousedown: move |evt| evt.prevent_default(),
                                                    for (s_idx, sug) in key_suggestions.iter().enumerate() {
                                                        button {
                                                            key: "hk-{i}-{sug}",
                                                            class: if s_idx == cur_ac_sel { "hdr-ac-item active" } else { "hdr-ac-item" },
                                                            r#type: "button",
                                                            onmousedown: {
                                                                let rows = rows.clone();
                                                                let picked = (*sug).to_string();
                                                                move |evt| {
                                                                    evt.prevent_default();
                                                                    let mut next = rows.clone();
                                                                    if let Some(r) = next.get_mut(i) {
                                                                        r.key = picked.clone();
                                                                    }
                                                                    ensure_trailing_empty(&mut next);
                                                                    on_change.call(next);
                                                                    ac_target.set(None);
                                                                }
                                                            },
                                                            span { "{sug}" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    td {
                                        div { class: "kv-cell-ac-wrap kv-val-cell",
                                            div {
                                                style: "flex: 1 1 auto; min-width: 0;",
                                                onfocusin: move |_| {
                                                    if is_headers {
                                                        ac_target.set(Some(HdrAcTarget::Value(i)));
                                                        ac_selected.set(0);
                                                    }
                                                },
                                                onfocusout: move |_| {
                                                    if ac_target() == Some(HdrAcTarget::Value(i)) {
                                                        ac_target.set(None);
                                                    }
                                                },
                                                VarHoverInput {
                                                    class: "kv-var-input".to_string(),
                                                    value: row.value.clone(),
                                                    placeholder: if is_headers { "Value or {{var}}".to_string() } else { String::new() },
                                                    input_id: format!("np-kv-val-{i}"),
                                                    vars: vars.clone(),
                                                    on_input: {
                                                        let rows = rows.clone();
                                                        move |val: String| {
                                                            let mut next = rows.clone();
                                                            if let Some(r) = next.get_mut(i) {
                                                                r.value = val;
                                                            }
                                                            ensure_trailing_empty(&mut next);
                                                            on_change.call(next);
                                                            if is_headers {
                                                                ac_target.set(Some(HdrAcTarget::Value(i)));
                                                                ac_selected.set(0);
                                                            }
                                                        }
                                                    },
                                                    on_var_edit,
                                                }
                                            }
                                            if is_headers && !header_value_suggestions(&row.key, "").is_empty() {
                                                button {
                                                    class: "kv-ac-trigger-btn",
                                                    r#type: "button",
                                                    title: "Show suggested values for {row.key}",
                                                    onmousedown: move |evt| {
                                                        evt.prevent_default();
                                                        if ac_target() == Some(HdrAcTarget::Value(i)) {
                                                            ac_target.set(None);
                                                        } else {
                                                            ac_target.set(Some(HdrAcTarget::Value(i)));
                                                            ac_selected.set(0);
                                                        }
                                                    },
                                                    "▾"
                                                }
                                            }
                                            if !val_suggestions.is_empty() {
                                                div {
                                                    class: "hdr-ac-menu",
                                                    onmousedown: move |evt| evt.prevent_default(),
                                                    for (s_idx, sug) in val_suggestions.iter().enumerate() {
                                                        button {
                                                            key: "hv-{i}-{sug}",
                                                            class: if s_idx == cur_ac_sel { "hdr-ac-item active" } else { "hdr-ac-item" },
                                                            r#type: "button",
                                                            onmousedown: {
                                                                let rows = rows.clone();
                                                                let picked = (*sug).to_string();
                                                                move |evt| {
                                                                    evt.prevent_default();
                                                                    let mut next = rows.clone();
                                                                    if let Some(r) = next.get_mut(i) {
                                                                        r.value = picked.clone();
                                                                    }
                                                                    ensure_trailing_empty(&mut next);
                                                                    on_change.call(next);
                                                                    ac_target.set(None);
                                                                }
                                                            },
                                                            span { "{sug}" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    td { class: "kv-col-action",
                                        if !is_trailing_empty {
                                            button {
                                                class: "kv-delete-btn",
                                                r#type: "button",
                                                title: "Delete row",
                                                onclick: {
                                                    let rows = rows.clone();
                                                    move |_| {
                                                        let mut next = rows.clone();
                                                        if i < next.len() {
                                                            next.remove(i);
                                                        }
                                                        ensure_trailing_empty(&mut next);
                                                        on_change.call(next);
                                                        ac_target.set(None);
                                                    }
                                                },
                                                "✕"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn ensure_trailing_empty(rows: &mut Vec<KeyValue>) {
    let needs = rows
        .last()
        .map(|r| !r.key.is_empty() || !r.value.is_empty())
        .unwrap_or(true);
    if needs {
        rows.push(KeyValue::new("", ""));
    }
}

type BodyChangePayload = (
    BodyMode,
    String,
    String,
    Vec<KeyValue>,
    Vec<FormDataField>,
    String,
    String,
);

#[component]
fn BodyEditor(
    mode: BodyMode,
    body: String,
    binary_path: String,
    urlencoded: Vec<KeyValue>,
    formdata: Vec<FormDataField>,
    graphql_query: String,
    graphql_variables: String,
    vars: VariableContext,
    #[props(default)] data_root: String,
    on_var_edit: EventHandler<VarEditEvent>,
    on_change: EventHandler<BodyChangePayload>,
) -> Element {
    let mode_str = match mode {
        BodyMode::None => "none",
        BodyMode::Raw => "raw",
        BodyMode::Urlencoded => "urlencoded",
        BodyMode::FormData => "formdata",
        BodyMode::Graphql => "graphql",
        BodyMode::Binary => "binary",
    };
    rsx! {
        select {
            value: "{mode_str}",
            onchange: {
                let body = body.clone();
                let binary_path = binary_path.clone();
                let urlencoded = urlencoded.clone();
                let formdata = formdata.clone();
                let graphql_query = graphql_query.clone();
                let graphql_variables = graphql_variables.clone();
                move |evt: Event<FormData>| {
                    let mode = match evt.value().as_str() {
                        "raw" => BodyMode::Raw,
                        "urlencoded" => BodyMode::Urlencoded,
                        "formdata" => BodyMode::FormData,
                        "graphql" => BodyMode::Graphql,
                        "binary" => BodyMode::Binary,
                        _ => BodyMode::None,
                    };
                    on_change.call((
                        mode,
                        body.clone(),
                        binary_path.clone(),
                        urlencoded.clone(),
                        formdata.clone(),
                        graphql_query.clone(),
                        graphql_variables.clone(),
                    ));
                }
            },
            option { value: "none", "none" }
            option { value: "raw", "raw (JSON / text)" }
            option { value: "urlencoded", "x-www-form-urlencoded" }
            option { value: "formdata", "form-data" }
            option { value: "graphql", "GraphQL" }
            option { value: "binary", "binary (file)" }
        }
        if mode == BodyMode::Raw {
            div { class: "body-editor-toolbar",
                div { class: "body-editor-tools-left",
                    button {
                        class: "btn btn-sm btn-ghost",
                        r#type: "button",
                        title: "Format and indent JSON body",
                        onclick: {
                            let body = body.clone();
                            let binary_path = binary_path.clone();
                            let urlencoded = urlencoded.clone();
                            let formdata = formdata.clone();
                            let graphql_query = graphql_query.clone();
                            let graphql_variables = graphql_variables.clone();
                            move |_| {
                                if let Some(pretty) = crate::core::beautify_json_body(&body) {
                                    on_change.call((
                                        mode,
                                        pretty,
                                        binary_path.clone(),
                                        urlencoded.clone(),
                                        formdata.clone(),
                                        graphql_query.clone(),
                                        graphql_variables.clone(),
                                    ));
                                }
                            }
                        },
                        super::icons::ActionIcon { name: "format", size: 15 }
                        span { "Beautify JSON" }
                    }
                }
                div { class: "body-editor-tools-right",
                    match crate::core::lint_json_body(&body) {
                        Ok(()) if !body.trim().is_empty() => rsx! {
                            span { class: "json-lint-badge json-lint-ok", "✓ Valid JSON" }
                        },
                        Err((line, col, msg)) => rsx! {
                            span {
                                class: "json-lint-badge json-lint-err",
                                title: "Line {line}, Column {col}: {msg}",
                                "⚠ Ln {line}, Col {col}: {msg}"
                            }
                        },
                        _ => rsx! {}
                    }
                }
            }
            MultilineCodeField {
                class: "body-editor".to_string(),
                value: body.clone(),
                placeholder: "{\"hello\": \"world\"}".to_string(),
                vars: Some(vars.clone()),
                syntax: EditorSyntax::Json,
                on_change: {
                    let binary_path = binary_path.clone();
                    let urlencoded = urlencoded.clone();
                    let formdata = formdata.clone();
                    let graphql_query = graphql_query.clone();
                    let graphql_variables = graphql_variables.clone();
                    move |text: String| on_change.call((
                        mode,
                        text,
                        binary_path.clone(),
                        urlencoded.clone(),
                        formdata.clone(),
                        graphql_query.clone(),
                        graphql_variables.clone(),
                    ))
                },
            }
        } else if mode == BodyMode::Binary {
            label { class: "hint", "Local file path (raw bytes on send) or workspace assets path" }
            input {
                class: "url-input",
                value: "{binary_path}",
                placeholder: "assets/payloads/file.bin or /path/to/file.bin",
                oninput: {
                    let body = body.clone();
                    let urlencoded = urlencoded.clone();
                    let formdata = formdata.clone();
                    let graphql_query = graphql_query.clone();
                    let graphql_variables = graphql_variables.clone();
                    move |evt| on_change.call((
                        BodyMode::Binary,
                        body.clone(),
                        evt.value(),
                        urlencoded.clone(),
                        formdata.clone(),
                        graphql_query.clone(),
                        graphql_variables.clone(),
                    ))
                }
            }
            div { class: "btn-group", style: "display: flex; gap: 8px; margin-top: 6px;",
                button {
                    class: "btn btn-ghost",
                    onclick: {
                        let body = body.clone();
                        let urlencoded = urlencoded.clone();
                        let formdata = formdata.clone();
                        let graphql_query = graphql_query.clone();
                        let graphql_variables = graphql_variables.clone();
                        move |_| {
                            if let Some(p) = rfd::FileDialog::new().pick_file() {
                                on_change.call((
                                    BodyMode::Binary,
                                    body.clone(),
                                    p.display().to_string(),
                                    urlencoded.clone(),
                                    formdata.clone(),
                                    graphql_query.clone(),
                                    graphql_variables.clone(),
                                ));
                            }
                        }
                    },
                    "Browse…"
                }
                button {
                    class: "btn btn-ghost",
                    title: "Select file and save copy to workspace assets/ directory for git sync",
                    onclick: {
                        let body = body.clone();
                        let urlencoded = urlencoded.clone();
                        let formdata = formdata.clone();
                        let graphql_query = graphql_query.clone();
                        let graphql_variables = graphql_variables.clone();
                        let data_root = data_root.clone();
                        move |_| {
                            if let Some(p) = rfd::FileDialog::new().pick_file() {
                                let path_str = if !data_root.is_empty() {
                                    crate::core::copy_file_to_assets(
                                        std::path::Path::new(&data_root),
                                        &p,
                                        Some("payloads"),
                                    ).unwrap_or_else(|_| p.display().to_string())
                                } else {
                                    p.display().to_string()
                                };
                                on_change.call((
                                    BodyMode::Binary,
                                    body.clone(),
                                    path_str,
                                    urlencoded.clone(),
                                    formdata.clone(),
                                    graphql_query.clone(),
                                    graphql_variables.clone(),
                                ));
                            }
                        }
                    },
                    "↑ Upload to Assets…"
                }
            }
        } else if mode == BodyMode::Urlencoded {
            KvEditor {
                rows: urlencoded.clone(),
                vars: vars.clone(),
                on_var_edit,
                on_change: {
                    let body = body.clone();
                    let binary_path = binary_path.clone();
                    let formdata = formdata.clone();
                    let graphql_query = graphql_query.clone();
                    let graphql_variables = graphql_variables.clone();
                    move |rows| on_change.call((
                        BodyMode::Urlencoded,
                        body.clone(),
                        binary_path.clone(),
                        rows,
                        formdata.clone(),
                        graphql_query.clone(),
                        graphql_variables.clone(),
                    ))
                }
            }
        } else if mode == BodyMode::FormData {
            p { class: "hint", "Check File? and click Upload or put a path in Value to attach files." }
            FormDataEditor {
                rows: formdata.clone(),
                vars: vars.clone(),
                data_root: data_root.clone(),
                on_var_edit,
                on_change: {
                    let body = body.clone();
                    let binary_path = binary_path.clone();
                    let urlencoded = urlencoded.clone();
                    let graphql_query = graphql_query.clone();
                    let graphql_variables = graphql_variables.clone();
                    move |rows| on_change.call((
                        BodyMode::FormData,
                        body.clone(),
                        binary_path.clone(),
                        urlencoded.clone(),
                        rows,
                        graphql_query.clone(),
                        graphql_variables.clone(),
                    ))
                }
            }
        } else if mode == BodyMode::Graphql {
            label { class: "hint", "Query" }
            CommentableTextarea {
                class: "body-editor".to_string(),
                value: graphql_query.clone(),
                placeholder: "query {{ viewer {{ login }} }}".to_string(),
                vars: Some(vars.clone()),
                on_change: {
                    let body = body.clone();
                    let binary_path = binary_path.clone();
                    let urlencoded = urlencoded.clone();
                    let formdata = formdata.clone();
                    let graphql_variables = graphql_variables.clone();
                    move |text: String| on_change.call((
                        BodyMode::Graphql,
                        body.clone(),
                        binary_path.clone(),
                        urlencoded.clone(),
                        formdata.clone(),
                        text,
                        graphql_variables.clone(),
                    ))
                },
            }
            label { class: "hint", "Variables (JSON)" }
            CommentableTextarea {
                class: "body-editor".to_string(),
                value: graphql_variables.clone(),
                placeholder: "{}".to_string(),
                vars: Some(vars.clone()),
                on_change: {
                    let body = body.clone();
                    let binary_path = binary_path.clone();
                    let urlencoded = urlencoded.clone();
                    let formdata = formdata.clone();
                    let graphql_query = graphql_query.clone();
                    move |text: String| on_change.call((
                        BodyMode::Graphql,
                        body.clone(),
                        binary_path.clone(),
                        urlencoded.clone(),
                        formdata.clone(),
                        graphql_query.clone(),
                        text,
                    ))
                },
            }
        } else {
            p { class: "hint", "No body." }
        }
    }
}

#[component]
fn FormDataEditor(
    rows: Vec<FormDataField>,
    vars: VariableContext,
    #[props(default)] data_root: String,
    on_var_edit: EventHandler<VarEditEvent>,
    on_change: EventHandler<Vec<FormDataField>>,
) -> Element {
    rsx! {
        table { class: "kv-table",
            thead {
                tr {
                    th { "Key" }
                    th { "Value / File path" }
                    th { "File?" }
                    th { "Upload / Select" }
                }
            }
            tbody {
                for (i, row) in rows.iter().enumerate() {
                    tr { key: "fd-{i}",
                        td {
                            input {
                                r#type: "text",
                                value: "{row.key}",
                                oninput: {
                                    let rows = rows.clone();
                                    move |evt: Event<FormData>| {
                                        let mut next = rows.clone();
                                        if let Some(r) = next.get_mut(i) {
                                            r.key = evt.value();
                                        }
                                        ensure_trailing_form(&mut next);
                                        on_change.call(next);
                                    }
                                }
                            }
                        }
                        td {
                            VarHoverInput {
                                class: "kv-var-input".to_string(),
                                value: row.value.clone(),
                                placeholder: if row.is_file {
                                    String::from("assets/payloads/... or /path/to/file")
                                } else {
                                    String::from("value")
                                },
                                input_id: format!("np-form-val-{i}"),
                                vars: vars.clone(),
                                on_input: {
                                    let rows = rows.clone();
                                    move |val: String| {
                                        let mut next = rows.clone();
                                        if let Some(r) = next.get_mut(i) {
                                            r.value = val;
                                        }
                                        ensure_trailing_form(&mut next);
                                        on_change.call(next);
                                    }
                                },
                                on_var_edit,
                            }
                        }
                        td {
                            input {
                                r#type: "checkbox",
                                checked: row.is_file,
                                onchange: {
                                    let rows = rows.clone();
                                    move |evt: Event<FormData>| {
                                        let mut next = rows.clone();
                                        if let Some(r) = next.get_mut(i) {
                                            r.is_file = evt.checked();
                                        }
                                        on_change.call(next);
                                    }
                                }
                            }
                        }
                        td {
                            if row.is_file {
                                div { class: "kv-upload-actions", style: "display: flex; gap: 4px;",
                                    button {
                                        class: "btn btn-ghost",
                                        title: "Browse local file",
                                        onclick: {
                                            let rows = rows.clone();
                                            move |_| {
                                                if let Some(p) = rfd::FileDialog::new().pick_file() {
                                                    let mut next = rows.clone();
                                                    if let Some(r) = next.get_mut(i) {
                                                        r.value = p.display().to_string();
                                                    }
                                                    ensure_trailing_form(&mut next);
                                                    on_change.call(next);
                                                }
                                            }
                                        },
                                        "Browse…"
                                    }
                                    button {
                                        class: "btn btn-ghost",
                                        title: "Upload & copy file to workspace assets/payloads/ directory for git sync",
                                        onclick: {
                                            let rows = rows.clone();
                                            let data_root = data_root.clone();
                                            move |_| {
                                                if let Some(p) = rfd::FileDialog::new().pick_file() {
                                                    let path_str = if !data_root.is_empty() {
                                                        crate::core::copy_file_to_assets(
                                                            std::path::Path::new(&data_root),
                                                            &p,
                                                            Some("payloads"),
                                                        ).unwrap_or_else(|_| p.display().to_string())
                                                    } else {
                                                        p.display().to_string()
                                                    };
                                                    let mut next = rows.clone();
                                                    if let Some(r) = next.get_mut(i) {
                                                        r.value = path_str;
                                                    }
                                                    ensure_trailing_form(&mut next);
                                                    on_change.call(next);
                                                }
                                            }
                                        },
                                        "↑ Upload"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn ensure_trailing_form(rows: &mut Vec<FormDataField>) {
    let needs = rows
        .last()
        .map(|r| !r.key.is_empty() || !r.value.is_empty())
        .unwrap_or(true);
    if needs {
        rows.push(FormDataField::default());
    }
}

#[component]
pub(crate) fn AuthEditor(
    auth: AuthConfig,
    ancestor_origin: Option<crate::core::AncestorAuthOrigin>,
    #[props(default)] vars: Option<VariableContext>,
    #[props(default)] on_var_edit: Option<EventHandler<VarEditEvent>>,
    on_edit_parent_auth: EventHandler<uuid::Uuid>,
    on_change: EventHandler<AuthConfig>,
) -> Element {
    let auth_type = auth.auth_type;
    let vctx_opt = vars.clone();
    let token_ph = if auth_type == AuthType::ApiKey { "API key" } else { "Token (or secret://…)" };
    let auth_badge_text = match auth_type {
        AuthType::Inherit => "Inherits auth from parent folder or collection",
        AuthType::NoAuth => "No authorization header attached",
        AuthType::Bearer => "Sends Authorization: Bearer <token>",
        AuthType::Basic => "Sends Base64 HTTP Basic credentials",
        AuthType::ApiKey => "Injects custom API key header",
        AuthType::Digest => "HTTP Digest challenge-response",
        AuthType::Oauth2 => "OAuth 2.0 Bearer access token",
        AuthType::Oauth1 => "HMAC-SHA1 OAuth 1.0a signature",
        AuthType::Hawk => "Hawk MAC authentication header",
        AuthType::Ntlm => "Windows NTLM negotiation",
        AuthType::AwsSigV4 => "AWS Signature Version 4",
        AuthType::EdgeGrid => "Akamai EdgeGrid authentication",
    };
    rsx! {
        div { class: "auth-type-bar",
            span { class: "auth-type-field-label", "Authorization Type" }
            div { class: "auth-type-select-wrap",
                select {
                    class: "auth-type-select",
                    aria_label: "Authorization type",
                    value: "{auth_type_str(auth_type)}",
                    onchange: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.auth_type = parse_auth(&evt.value());
                            on_change.call(a.clone());
                        }
                    },
                    option { value: "inherit", "Inherit from parent" }
                    option { value: "no_auth", "No Auth" }
                    option { value: "bearer", "Bearer Token" }
                    option { value: "basic", "Basic Auth" }
                    option { value: "api_key", "API Key" }
                    option { value: "digest", "Digest" }
                    option { value: "oauth2", "OAuth 2.0" }
                    option { value: "oauth1", "OAuth 1.0" }
                    option { value: "hawk", "Hawk" }
                    option { value: "ntlm", "NTLM" }
                    option { value: "aws_sigv4", "AWS Signature" }
                    option { value: "edgegrid", "Akamai EdgeGrid" }
                }
                span { class: "auth-type-caret", "aria-hidden": "true", "▾" }
            }
            span { class: "auth-type-summary", "{auth_badge_text}" }
        }
        if matches!(auth_type, AuthType::Bearer | AuthType::ApiKey) {
            if let Some(vctx) = &vctx_opt {
                VarHoverInput {
                    class: "kv-var-input".to_string(),
                    value: auth.token.clone(),
                    placeholder: token_ph.to_string(),
                    input_id: "auth-token-input".to_string(),
                    vars: vctx.clone(),
                    on_input: {
                        let mut a = auth.clone();
                        move |val: String| {
                            a.token = val;
                            on_change.call(a.clone());
                        }
                    },
                    on_var_edit: move |evt| {
                        if let Some(h) = on_var_edit {
                            h.call(evt);
                        }
                    },
                }
            } else {
                input {
                    r#type: "text",
                    placeholder: if auth_type == AuthType::ApiKey { "API key" } else { "Token (or secret://…)" },
                    value: "{auth.token}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.token = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
            }
        }
        if auth_type == AuthType::ApiKey {
            if let Some(vctx) = &vctx_opt {
                VarHoverInput {
                    class: "kv-var-input".to_string(),
                    value: auth.api_key_header.clone(),
                    placeholder: "Header name (default X-Api-Key)".to_string(),
                    input_id: "auth-apikey-header-input".to_string(),
                    vars: vctx.clone(),
                    on_input: {
                        let mut a = auth.clone();
                        move |val: String| {
                            a.api_key_header = val;
                            on_change.call(a.clone());
                        }
                    },
                    on_var_edit: move |evt| {
                        if let Some(h) = on_var_edit {
                            h.call(evt);
                        }
                    },
                }
            } else {
                input {
                    r#type: "text",
                    placeholder: "Header name (default X-Api-Key)",
                    value: "{auth.api_key_header}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.api_key_header = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
            }
        }
        if matches!(auth_type, AuthType::Basic | AuthType::Digest) {
            if let Some(vctx) = &vctx_opt {
                VarHoverInput {
                    class: "kv-var-input".to_string(),
                    value: auth.username.clone(),
                    placeholder: "Username".to_string(),
                    input_id: "auth-username-input".to_string(),
                    vars: vctx.clone(),
                    on_input: {
                        let mut a = auth.clone();
                        move |val: String| {
                            a.username = val;
                            on_change.call(a.clone());
                        }
                    },
                    on_var_edit: move |evt| {
                        if let Some(h) = on_var_edit {
                            h.call(evt);
                        }
                    },
                }
                VarHoverInput {
                    class: "kv-var-input".to_string(),
                    value: auth.password.clone(),
                    placeholder: "Password".to_string(),
                    input_id: "auth-password-input".to_string(),
                    vars: vctx.clone(),
                    on_input: {
                        let mut a = auth.clone();
                        move |val: String| {
                            a.password = val;
                            on_change.call(a.clone());
                        }
                    },
                    on_var_edit: move |evt| {
                        if let Some(h) = on_var_edit {
                            h.call(evt);
                        }
                    },
                }
            } else {
                input {
                    r#type: "text",
                    placeholder: "Username",
                    value: "{auth.username}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.username = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
                input {
                    r#type: "password",
                    placeholder: "Password",
                    value: "{auth.password}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.password = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
            }
        }
        if auth_type == AuthType::Oauth2 {
            div { class: "auth-type-select-wrap",
                select {
                    class: "auth-type-select",
                    aria_label: "OAuth 2.0 Grant Type",
                    value: if auth.oauth2.grant == OAuth2Grant::AuthorizationCode { "auth_code" } else { "client_creds" },
                onchange: {
                    let mut a = auth.clone();
                    move |evt: Event<FormData>| {
                        a.oauth2.grant = if evt.value() == "auth_code" {
                            OAuth2Grant::AuthorizationCode
                        } else {
                            OAuth2Grant::ClientCredentials
                        };
                        on_change.call(a.clone());
                    }
                },
                    option { value: "client_creds", "Client Credentials" }
                    option { value: "auth_code", "Authorization Code" }
                }
                span { class: "auth-type-caret", "aria-hidden": "true", "▾" }
            }
            if let Some(vctx) = &vctx_opt {
                VarHoverInput {
                    class: "kv-var-input".to_string(),
                    value: auth.oauth2.auth_url.clone(),
                    placeholder: "Auth URL".to_string(),
                    input_id: "auth-oauth2-authurl-input".to_string(),
                    vars: vctx.clone(),
                    on_input: {
                        let mut a = auth.clone();
                        move |val: String| {
                            a.oauth2.auth_url = val;
                            on_change.call(a.clone());
                        }
                    },
                    on_var_edit: move |evt| {
                        if let Some(h) = on_var_edit {
                            h.call(evt);
                        }
                    },
                }
                VarHoverInput {
                    class: "kv-var-input".to_string(),
                    value: auth.oauth2.token_url.clone(),
                    placeholder: "Token URL".to_string(),
                    input_id: "auth-oauth2-tokenurl-input".to_string(),
                    vars: vctx.clone(),
                    on_input: {
                        let mut a = auth.clone();
                        move |val: String| {
                            a.oauth2.token_url = val;
                            on_change.call(a.clone());
                        }
                    },
                    on_var_edit: move |evt| {
                        if let Some(h) = on_var_edit {
                            h.call(evt);
                        }
                    },
                }
                VarHoverInput {
                    class: "kv-var-input".to_string(),
                    value: auth.oauth2.client_id.clone(),
                    placeholder: "Client ID".to_string(),
                    input_id: "auth-oauth2-clientid-input".to_string(),
                    vars: vctx.clone(),
                    on_input: {
                        let mut a = auth.clone();
                        move |val: String| {
                            a.oauth2.client_id = val;
                            on_change.call(a.clone());
                        }
                    },
                    on_var_edit: move |evt| {
                        if let Some(h) = on_var_edit {
                            h.call(evt);
                        }
                    },
                }
                VarHoverInput {
                    class: "kv-var-input".to_string(),
                    value: auth.oauth2.client_secret.clone(),
                    placeholder: "Client Secret".to_string(),
                    input_id: "auth-oauth2-clientsecret-input".to_string(),
                    vars: vctx.clone(),
                    on_input: {
                        let mut a = auth.clone();
                        move |val: String| {
                            a.oauth2.client_secret = val;
                            on_change.call(a.clone());
                        }
                    },
                    on_var_edit: move |evt| {
                        if let Some(h) = on_var_edit {
                            h.call(evt);
                        }
                    },
                }
                VarHoverInput {
                    class: "kv-var-input".to_string(),
                    value: auth.oauth2.scope.clone(),
                    placeholder: "Scope".to_string(),
                    input_id: "auth-oauth2-scope-input".to_string(),
                    vars: vctx.clone(),
                    on_input: {
                        let mut a = auth.clone();
                        move |val: String| {
                            a.oauth2.scope = val;
                            on_change.call(a.clone());
                        }
                    },
                    on_var_edit: move |evt| {
                        if let Some(h) = on_var_edit {
                            h.call(evt);
                        }
                    },
                }
            } else {
                input {
                    r#type: "text",
                    placeholder: "Auth URL",
                    value: "{auth.oauth2.auth_url}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.oauth2.auth_url = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
                input {
                    r#type: "text",
                    placeholder: "Token URL",
                    value: "{auth.oauth2.token_url}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.oauth2.token_url = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
                input {
                    r#type: "text",
                    placeholder: "Client ID",
                    value: "{auth.oauth2.client_id}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.oauth2.client_id = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
                input {
                    r#type: "password",
                    placeholder: "Client Secret",
                    value: "{auth.oauth2.client_secret}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.oauth2.client_secret = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
                input {
                    r#type: "text",
                    placeholder: "Scope",
                    value: "{auth.oauth2.scope}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.oauth2.scope = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
            }
            input {
                r#type: "text",
                placeholder: "Redirect URI (auth code)",
                value: "{auth.oauth2.redirect_uri}",
                oninput: {
                    let mut a = auth.clone();
                    move |evt: Event<FormData>| {
                        a.oauth2.redirect_uri = evt.value();
                        on_change.call(a.clone());
                    }
                }
            }
            p { class: "hint",
                "Access token: "
                code { "{auth.oauth2.access_token}" }
            }
            button {
                class: "btn btn-ghost",
                onclick: {
                    let a = auth.clone();
                    move |_| {
                        let a = a.clone();
                        spawn(async move {
                            let (tx, rx) = tokio::sync::oneshot::channel();
                            let mut oauth2_cfg = a.oauth2.clone();
                            std::thread::spawn(move || {
                                let res = fetch_oauth2_token(&mut oauth2_cfg);
                                let _ = tx.send((res, oauth2_cfg));
                            });
                            if let Ok((res, updated_oauth2)) = rx.await {
                                let mut next_auth = a.clone();
                                match res {
                                    Ok(()) => {
                                        next_auth.oauth2 = updated_oauth2;
                                        on_change.call(next_auth);
                                    }
                                    Err(e) => {
                                        next_auth.oauth2.access_token = format!("ERROR: {e}");
                                        on_change.call(next_auth);
                                    }
                                }
                            }
                        });
                    }
                },
                "Get New Access Token"
            }
        }
        if auth_type == AuthType::Inherit {
            match ancestor_origin {
                Some(origin) => {
                    let origin_id = origin.id;
                    let origin_name = origin.name.clone();
                    let origin_kind = if origin.is_collection { "Collection" } else { "Folder" };
                    let auth_kind = format!("{:?}", origin.auth.auth_type);
                    let preview = match origin.auth.auth_type {
                        AuthType::Bearer => {
                            let t = &origin.auth.token;
                            let char_count = t.chars().count();
                            if char_count > 8 {
                                let head: String = t.chars().take(4).collect();
                                let tail: String = t.chars().skip(char_count.saturating_sub(4)).collect();
                                format!("{head}…{tail}")
                            } else if !t.is_empty() {
                                "••••••••".into()
                            } else {
                                "(empty token)".into()
                            }
                        }
                        AuthType::Basic => format!("User: {}", origin.auth.username),
                        AuthType::ApiKey => format!("Header: {}", if origin.auth.api_key_header.is_empty() { "X-Api-Key" } else { &origin.auth.api_key_header }),
                        _ => auth_kind.clone(),
                    };
                    rsx! {
                        div { class: "auth-inherited-card",
                            div { class: "auth-inherited-header",
                                div { class: "auth-inherited-origin",
                                    span { class: "auth-inherited-badge", "{origin_kind}" }
                                    span { "Inheriting from “{origin_name}”" }
                                }
                                button {
                                    class: "btn btn-ghost btn-sm",
                                    onclick: move |_| on_edit_parent_auth.call(origin_id),
                                    "Edit {origin_kind} Auth"
                                }
                            }
                            div { class: "auth-inherited-details",
                                span { "Method: {auth_kind}" }
                                span { "Preview: " span { class: "auth-token-preview", "{preview}" } }
                            }
                        }
                    }
                }
                None => rsx! {
                    div { class: "auth-inherited-card",
                        div { class: "auth-inherited-header",
                            span { class: "hint", "No parent collection or folder has configured authentication." }
                        }
                    }
                }
            }
        }
        if auth_type == AuthType::Digest {
            p { class: "hint", "Digest: MD5 / qop=auth challenge after 401." }
        }
        if matches!(
            auth_type,
            AuthType::Oauth1 | AuthType::Hawk | AuthType::AwsSigV4 | AuthType::EdgeGrid | AuthType::Ntlm
        ) {
            if auth_type == AuthType::Ntlm {
                input {
                    r#type: "text",
                    placeholder: "Username (DOMAIN\\user)",
                    value: "{auth.username}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.username = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
                input {
                    r#type: "password",
                    placeholder: "Password",
                    value: "{auth.password}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.password = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
                p { class: "hint", "NTLM: Type1 negotiate only (partial — no full 401 handshake)." }
            } else {
                input {
                    r#type: "text",
                    placeholder: match auth_type {
                        AuthType::Oauth1 => "Consumer Key",
                        AuthType::Hawk => "Hawk ID",
                        AuthType::AwsSigV4 => "Access Key ID",
                        AuthType::EdgeGrid => "Client Token",
                        _ => "Key",
                    },
                    value: "{auth.extra.key}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.extra.key = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
                input {
                    r#type: "password",
                    placeholder: match auth_type {
                        AuthType::Oauth1 => "Consumer Secret",
                        AuthType::Hawk => "Hawk Key",
                        AuthType::AwsSigV4 => "Secret Access Key",
                        AuthType::EdgeGrid => "Client Secret",
                        _ => "Secret",
                    },
                    value: "{auth.extra.secret}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.extra.secret = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
                input {
                    r#type: "text",
                    placeholder: match auth_type {
                        AuthType::Oauth1 => "Token",
                        AuthType::AwsSigV4 => "Session token (optional)",
                        AuthType::EdgeGrid => "Access Token",
                        _ => "Token (optional)",
                    },
                    value: "{auth.extra.token}",
                    oninput: {
                        let mut a = auth.clone();
                        move |evt: Event<FormData>| {
                            a.extra.token = evt.value();
                            on_change.call(a.clone());
                        }
                    }
                }
                if matches!(auth_type, AuthType::Oauth1 | AuthType::EdgeGrid) {
                    input {
                        r#type: "text",
                        placeholder: if auth_type == AuthType::Oauth1 {
                            "Token Secret"
                        } else {
                            "Host (optional)"
                        },
                        value: "{auth.extra.token_secret}",
                        oninput: {
                            let mut a = auth.clone();
                            move |evt: Event<FormData>| {
                                a.extra.token_secret = evt.value();
                                on_change.call(a.clone());
                            }
                        }
                    }
                }
                if auth_type == AuthType::AwsSigV4 {
                    input {
                        r#type: "text",
                        placeholder: "Region (us-east-1)",
                        value: "{auth.extra.region}",
                        oninput: {
                            let mut a = auth.clone();
                            move |evt: Event<FormData>| {
                                a.extra.region = evt.value();
                                on_change.call(a.clone());
                            }
                        }
                    }
                    input {
                        r#type: "text",
                        placeholder: "Service (execute-api)",
                        value: "{auth.extra.service}",
                        oninput: {
                            let mut a = auth.clone();
                            move |evt: Event<FormData>| {
                                a.extra.service = evt.value();
                                on_change.call(a.clone());
                            }
                        }
                    }
                }
            }
        }
        p { class: "hint", "🔒 Security: Use {{{{vault:key}}}} to inject secrets from your local vault.json or OS Keychain without committing credentials to Git." }
    }
}

fn auth_type_str(t: AuthType) -> &'static str {
    match t {
        AuthType::NoAuth => "no_auth",
        AuthType::Inherit => "inherit",
        AuthType::Bearer => "bearer",
        AuthType::Basic => "basic",
        AuthType::ApiKey => "api_key",
        AuthType::Digest => "digest",
        AuthType::Oauth2 => "oauth2",
        AuthType::Oauth1 => "oauth1",
        AuthType::Hawk => "hawk",
        AuthType::Ntlm => "ntlm",
        AuthType::AwsSigV4 => "aws_sigv4",
        AuthType::EdgeGrid => "edgegrid",
    }
}

fn parse_auth(s: &str) -> AuthType {
    match s {
        "bearer" => AuthType::Bearer,
        "basic" => AuthType::Basic,
        "api_key" => AuthType::ApiKey,
        "digest" => AuthType::Digest,
        "oauth2" => AuthType::Oauth2,
        "oauth1" => AuthType::Oauth1,
        "hawk" => AuthType::Hawk,
        "ntlm" => AuthType::Ntlm,
        "aws_sigv4" => AuthType::AwsSigV4,
        "edgegrid" => AuthType::EdgeGrid,
        "inherit" => AuthType::Inherit,
        _ => AuthType::NoAuth,
    }
}

fn append_script_snippet(existing: &str, snippet: &str) -> String {
    let mut next = existing.to_string();
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(snippet);
    if !next.ends_with('\n') {
        next.push('\n');
    }
    next
}

#[component]
pub(crate) fn ScriptsEditor(
    pre: String,
    test: String,
    vars: VariableContext,
    on_change: EventHandler<(String, String)>,
) -> Element {
    rsx! {
        p { class: "hint",
            "Supported: blitz.variables|environment|collectionVariables|globals (.get/set/unset/has/toObject/clear, replaceIn), vault, cookies (get/set/clear), request (url.getPath, method, headers.upsert, body), response (code/status/responseTime/json/text/headers, to.have.status/jsonBody), test/expect (equal/eql/include/ok/above/below/a/property/not), sendRequest, execution, info, visualizer, console.*, CryptoJS.SHA256/HmacSHA256 (pm & epm aliases supported). require: crypto-js only. No async tests / npm / uvm."
        }
        label { "Pre-request" }
        ScriptCodeEditor {
            class: "body-editor".to_string(),
            value: pre.clone(),
            placeholder: "blitz.environment.set(\"baseUrl\", \"https://httpbin.org\");\nconsole.log(\"pre\");".to_string(),
            vars: vars.clone(),
            on_change: {
                let test = test.clone();
                move |text: String| on_change.call((text, test.clone()))
            }
        }
        label { "Tests (post-response)" }
        div { class: "kv-actions",
            button {
                class: "btn btn-ghost",
                r#type: "button",
                onclick: {
                    let pre = pre.clone();
                    let test = test.clone();
                    move |_| {
                        on_change.call((
                            pre.clone(),
                            append_script_snippet(
                                &test,
                                "blitz.test(\"Status 200\", function () {\n  blitz.response.to.have.status(200);\n});\n",
                            ),
                        ))
                    }
                },
                "Status 200"
            }
            button {
                class: "btn btn-ghost",
                r#type: "button",
                onclick: {
                    let pre = pre.clone();
                    let test = test.clone();
                    move |_| {
                        on_change.call((
                            pre.clone(),
                            append_script_snippet(
                                &test,
                                "blitz.test(\"Save token\", function () {\n  var t = blitz.response.json().data.token;\n  blitz.environment.set(\"token\", t);\n  blitz.collectionVariables.set(\"token\", t);\n});\n",
                            ),
                        ))
                    }
                },
                "Save token"
            }
            button {
                class: "btn btn-ghost",
                r#type: "button",
                onclick: {
                    let pre = pre.clone();
                    let test = test.clone();
                    move |_| {
                        on_change.call((
                            pre.clone(),
                            append_script_snippet(
                                &test,
                                "blitz.test(\"JSON ok\", function () {\n  blitz.response.to.have.jsonBody();\n  blitz.expect(blitz.response.json()).to.be.ok;\n});\n",
                            ),
                        ))
                    }
                },
                "JSON ok"
            }
            button {
                class: "btn btn-ghost",
                r#type: "button",
                onclick: {
                    let pre = pre.clone();
                    let test = test.clone();
                    move |_| {
                        on_change.call((
                            pre.clone(),
                            append_script_snippet(
                                &test,
                                "blitz.test(\"Response time < 200ms\", function () {\n  blitz.expect(blitz.response.responseTime).to.be.below(200);\n});\n",
                            ),
                        ))
                    }
                },
                "< 200ms"
            }
        }
        ScriptCodeEditor {
            class: "body-editor".to_string(),
            value: test.clone(),
            placeholder: "blitz.test(\"status ok\", function () {{\n  blitz.expect(blitz.response.code).to.equal(200);\n}});".to_string(),
            vars: vars.clone(),
            on_change: {
                let pre = pre.clone();
                move |text: String| on_change.call((pre.clone(), text))
            }
        }
    }
}

#[component]
fn RequestSettingsEditor(
    settings: RequestSettings,
    on_change: EventHandler<RequestSettings>,
) -> Element {
    rsx! {
        div { class: "settings-fields",
            label { "Timeout (ms, 0 = none)" }
            input {
                r#type: "number",
                value: "{settings.timeout_ms}",
                oninput: {
                    let settings = settings.clone();
                    move |evt: Event<FormData>| {
                        let mut s = settings.clone();
                        s.timeout_ms = evt.value().parse().unwrap_or(30_000);
                        on_change.call(s);
                    }
                }
            }
            label { class: "checkbox-row",
                input {
                    r#type: "checkbox",
                    checked: settings.follow_redirects,
                    onchange: {
                        let settings = settings.clone();
                        move |evt: Event<FormData>| {
                            let mut s = settings.clone();
                            s.follow_redirects = evt.checked();
                            on_change.call(s);
                        }
                    }
                }
                " Follow redirects"
            }
            label { class: "checkbox-row",
                input {
                    r#type: "checkbox",
                    checked: settings.ssl_verify,
                    onchange: {
                        let settings = settings.clone();
                        move |evt: Event<FormData>| {
                            let mut s = settings.clone();
                            s.ssl_verify = evt.checked();
                            on_change.call(s);
                        }
                    }
                }
                " SSL certificate verification"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_sse_log;

    #[test]
    fn parse_sse_log_handles_connecting_connected_streaming_and_stopped_states() {
        let connecting = vec![
            r#"SSE_META:{"state":"connecting","status":0,"status_text":"Connecting…","elapsed_ms":0,"count":0,"method":"GET","url":"http://127.0.0.1:9999/sse"}"#.to_string(),
        ];
        let (m, evs, err) = parse_sse_log(&connecting);
        let m = m.unwrap();
        assert_eq!(m.state, "connecting");
        assert_eq!(m.status, 0);
        assert!(evs.is_empty());
        assert!(err.is_none());

        let connected = vec![
            r#"SSE_META:{"state":"connected","status":200,"status_text":"OK","connect_ms":14,"elapsed_ms":14,"count":0,"method":"GET","url":"http://127.0.0.1:9999/sse"}"#.to_string(),
        ];
        let (m, evs, err) = parse_sse_log(&connected);
        let m = m.unwrap();
        assert_eq!(m.state, "connected");
        assert_eq!(m.status, 200);
        assert_eq!(m.connect_ms, Some(14));
        assert!(evs.is_empty());
        assert!(err.is_none());

        let streaming = vec![
            r#"SSE_META:{"state":"streaming","status":200,"status_text":"OK","connect_ms":14,"elapsed_ms":52,"ttfb_ms":52,"count":1,"method":"POST","url":"http://127.0.0.1:9999/chat"}"#.to_string(),
            r#"SSE_EVENT:{"index":1,"event":"delta","id":"1","elapsed_ms":52,"data":"hello"}"#.to_string(),
        ];
        let (m, evs, _) = parse_sse_log(&streaming);
        let m = m.unwrap();
        assert_eq!(m.state, "streaming");
        assert_eq!(m.ttfb_ms, Some(52));
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].event, "delta");
    }
}
