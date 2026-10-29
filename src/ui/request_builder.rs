use dioxus::prelude::*;

use crate::core::{
    fetch_oauth2_token, AuthConfig, AuthType, BodyMode, FormDataField, HttpMethod, KeyValue,
    OAuth2Grant, RequestDraft, RequestProtocol, RequestSettings, VariableContext,
};
use super::var_hover::{VarEditEvent, VarHoverInput};
use super::code_editor::{EditorSyntax, MultilineCodeField};
use super::{CommentableTextarea, ScriptCodeEditor};

pub use super::var_hover::VarEditEvent as RequestVarEditEvent;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BuilderTab {
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
    #[props(default)] ancestor_origin: Option<crate::core::AncestorAuthOrigin>,
    #[props(default)] on_edit_parent_auth: EventHandler<uuid::Uuid>,
    on_change: EventHandler<RequestDraft>,
    on_send: EventHandler<()>,
    /// Protocol actions: ws_connect | ws_send | ws_disconnect | mqtt_publish | mqtt_subscribe | mcp_list | mcp_call
    on_protocol_action: EventHandler<String>,
    sending: bool,
    ws_connected: bool,
    protocol_log: Vec<String>,
    vars: VariableContext,
    #[props(default)] data_root: String,
    on_var_edit: EventHandler<VarEditEvent>,
    #[props(default)] on_open_code: EventHandler<()>,
) -> Element {
    let mut tab = use_signal(|| BuilderTab::Params);

    let set_method = {
        let draft = draft.clone();
        move |evt: Event<FormData>| {
            let mut d = draft.clone();
            d.method = parse_method(&evt.value());
            on_change.call(d);
        }
    };

    let set_protocol = {
        let draft = draft.clone();
        move |evt: Event<FormData>| {
            let mut d = draft.clone();
            d.protocol = parse_protocol(&evt.value());
            on_change.call(d);
        }
    };

    let proto_class = protocol_class(draft.protocol);
    let method_class = method_class_name(draft.method);

    rsx! {
        div { class: "request-row",
            select {
                class: "method-select protocol-select protocol-{proto_class}",
                value: "{draft.protocol}",
                title: "Protocol",
                onchange: set_protocol,
                for p in RequestProtocol::all() {
                    option { value: "{p}", selected: *p == draft.protocol, "{p}" }
                }
            }
            if draft.protocol == RequestProtocol::Http {
                select {
                    class: "method-select method-{method_class}",
                    value: "{draft.method}",
                    title: "HTTP method",
                    onchange: set_method,
                    for m in HttpMethod::all() {
                        option { value: "{m}", selected: *m == draft.method, "{m}" }
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
            if draft.protocol == RequestProtocol::Http {
                button {
                    class: "btn btn-ghost",
                    style: "font-family: var(--mono); font-weight: 700; padding: 0 10px;",
                    title: "Generate Code Snippet (cURL, Python, TS, Rust…)",
                    onclick: move |_| on_open_code.call(()),
                    "</>"
                }
            }
            if draft.protocol == RequestProtocol::Http || draft.protocol == RequestProtocol::Grpc {
                button {
                    class: if sending { "btn btn-ghost btn-cancel" } else { "btn btn-send" },
                    title: if sending { "Cancel in-flight request" } else { "Send request (⌘↵)" },
                    onclick: move |_| on_send.call(()),
                    if sending {
                        span { class: "loading-spinner-cancel" }
                        span { "Cancel" }
                    } else if draft.protocol == RequestProtocol::Grpc {
                        "Invoke"
                    } else {
                        span { class: "action-icon action-icon-bolt", style: "width: 12px; height: 12px; margin-right: 4px;", aria_hidden: "true" }
                        span { "Send" }
                    }
                }
            }
        }

        if draft.protocol != RequestProtocol::Http {
            ProtocolFields {
                draft: draft.clone(),
                on_change,
                on_protocol_action,
                ws_connected,
                protocol_log: protocol_log.clone(),
                sending,
            }
        }

        if draft.protocol == RequestProtocol::Http {
        div { class: "builder-tabs",
            for (label, t) in [
                ("Params".to_string(), BuilderTab::Params),
                ("Headers".to_string(), BuilderTab::Headers),
                ("Body".to_string(), BuilderTab::Body),
                ("Auth".to_string(), BuilderTab::Auth),
                (
                    if draft.assertions.is_empty() {
                        "Tests".to_string()
                    } else {
                        format!("Tests ({})", draft.assertions.iter().filter(|a| a.enabled).count())
                    },
                    BuilderTab::Tests,
                ),
                ("Scripts".to_string(), BuilderTab::Scripts),
                ("Docs".to_string(), BuilderTab::Docs),
                ("Settings".to_string(), BuilderTab::Settings),
            ] {
                button {
                    class: if tab() == t { "builder-tab active" } else { "builder-tab" },
                    onclick: move |_| tab.set(t),
                    "{label}"
                }
            }
        }

        div { class: "builder-pane",
            match tab() {
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
                                            tr {
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
        } // HTTP-only builder tabs
    }
}

fn parse_method(s: &str) -> HttpMethod {
    match s {
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "PATCH" => HttpMethod::Patch,
        "DELETE" => HttpMethod::Delete,
        "HEAD" => HttpMethod::Head,
        "OPTIONS" => HttpMethod::Options,
        _ => HttpMethod::Get,
    }
}

fn parse_protocol(s: &str) -> RequestProtocol {
    match s {
        "WebSocket" => RequestProtocol::WebSocket,
        "gRPC" => RequestProtocol::Grpc,
        "MQTT" => RequestProtocol::Mqtt,
        "MCP" => RequestProtocol::Mcp,
        "Socket.IO" => RequestProtocol::SocketIo,
        "SSE" => RequestProtocol::Sse,
        "GraphQL-WS" => RequestProtocol::GraphqlWs,
        _ => RequestProtocol::Http,
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
    protocol_log: Vec<String>,
    sending: bool,
) -> Element {
    rsx! {
        div { class: "builder-pane protocol-pane",
            match draft.protocol {
                RequestProtocol::WebSocket => rsx! {
                    label { class: "hint", "Message" }
                    CommentableTextarea {
                        class: "body-editor".to_string(),
                        value: draft.ws_message.clone(),
                        rows: Some("3"),
                        on_change: {
                            let draft = draft.clone();
                            move |text: String| {
                                let mut d = draft.clone();
                                d.ws_message = text;
                                on_change.call(d);
                            }
                        }
                    }
                    div { class: "kv-actions",
                        button {
                            class: "btn btn-send",
                            disabled: ws_connected || sending,
                            onclick: move |_| on_protocol_action.call("ws_connect".into()),
                            "Connect"
                        }
                        button {
                            class: "btn btn-ghost",
                            disabled: !ws_connected,
                            onclick: move |_| on_protocol_action.call("ws_send".into()),
                            "Send message"
                        }
                        button {
                            class: "btn btn-ghost",
                            disabled: !ws_connected,
                            onclick: move |_| on_protocol_action.call("ws_disconnect".into()),
                            "Disconnect"
                        }
                        span {
                            class: if ws_connected { "ws-state connected" } else { "ws-state disconnected" },
                            if ws_connected { "Connected" } else { "Disconnected" }
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
                    label { class: "hint", "JSON body → google.protobuf.Struct" }
                    CommentableTextarea {
                        class: "body-editor".to_string(),
                        value: draft.body.clone(),
                        rows: Some("6"),
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
                    CommentableTextarea {
                        class: "body-editor".to_string(),
                        value: draft.body.clone(),
                        rows: Some("3"),
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
                    CommentableTextarea {
                        class: "body-editor".to_string(),
                        value: draft.mcp_tool_args_json.clone(),
                        rows: Some("4"),
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
                    CommentableTextarea {
                        class: "body-editor".to_string(),
                        value: draft.body.clone(),
                        rows: Some("4"),
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
                    p { class: "hint", "SSE GET stream — URL bar is EventSource endpoint." }
                    div { class: "kv-actions",
                        button {
                            class: "btn btn-send",
                            disabled: sending,
                            onclick: move |_| on_protocol_action.call("sse_listen".into()),
                            "Listen (max 20 events)"
                        }
                    }
                },
                RequestProtocol::GraphqlWs => rsx! {
                    label { class: "hint", "Subscription query" }
                    CommentableTextarea {
                        class: "body-editor".to_string(),
                        value: draft.graphql_ws_query.clone(),
                        rows: Some("6"),
                        placeholder: "subscription {{ … }}".to_string(),
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
                    CommentableTextarea {
                        class: "body-editor".to_string(),
                        value: draft.graphql_variables.clone(),
                        rows: Some("3"),
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
            if !protocol_log.is_empty() {
                div { class: "protocol-log",
                    h4 { "Protocol log" }
                    for line in protocol_log.iter() {
                        pre { class: "console-line", "{line}" }
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

#[component]
fn KvEditor(
    rows: Vec<KeyValue>,
    vars: VariableContext,
    #[props(default)] is_headers: bool,
    on_var_edit: EventHandler<VarEditEvent>,
    on_change: EventHandler<Vec<KeyValue>>,
) -> Element {
    let mut bulk_mode = use_signal(|| false);
    let mut bulk_text = use_signal(String::new);
    let mut show_system_headers = use_signal(|| false);

    rsx! {
        div { class: "kv-toolbar",
            div { style: "display: flex; gap: 8px; align-items: center;",
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
                if is_headers {
                    button {
                        class: "system-headers-toggle",
                        r#type: "button",
                        onclick: move |_| show_system_headers.set(!show_system_headers()),
                        if show_system_headers() {
                            "▾ Hide auto-generated headers (6)"
                        } else {
                            "▸ 6 auto-generated headers hidden"
                        }
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
                        th { "Key" }
                        th { "Value" }
                    }
                }
                tbody {
                    if is_headers && show_system_headers() {
                        for (k, v) in [
                            ("User-Agent", "PostmanRuntime/7.43.0"),
                            ("Accept", "*/*"),
                            ("Accept-Encoding", "gzip, deflate, br"),
                            ("Connection", "keep-alive"),
                            ("Cache-Control", "no-cache"),
                            ("Postman-Token", "<calculated at runtime>"),
                        ] {
                            tr { class: "system-header-row",
                                td {
                                    input {
                                        r#type: "text",
                                        readonly: true,
                                        value: "{k}",
                                        title: "Auto-generated Postman system header",
                                    }
                                }
                                td {
                                    input {
                                        r#type: "text",
                                        readonly: true,
                                        value: "{v}",
                                        title: "Auto-generated Postman system header",
                                    }
                                }
                            }
                        }
                    }
                    for (i, row) in rows.iter().enumerate() {
                        tr {
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
                                            ensure_trailing_empty(&mut next);
                                            on_change.call(next);
                                        }
                                    }
                                }
                            }
                            td {
                                VarHoverInput {
                                    class: "kv-var-input".to_string(),
                                    value: row.value.clone(),
                                    placeholder: String::new(),
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
                            let clean = crate::runtime::postman_defaults::strip_json_comments(&body);
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&clean) {
                                if let Ok(pretty) = serde_json::to_string_pretty(&val) {
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
                        }
                    },
                    span { class: "action-icon action-icon-format", aria_hidden: "true" }
                    span { "Beautify JSON" }
                }
                {
                    let trimmed = body.trim();
                    if trimmed.is_empty() {
                        rsx! {}
                    } else {
                        let clean = crate::runtime::postman_defaults::strip_json_comments(trimmed);
                        match serde_json::from_str::<serde_json::Value>(&clean) {
                            Ok(_) => rsx! {
                                span { class: "json-lint-badge json-lint-ok", "✓ Valid JSON" }
                            },
                            Err(e) => rsx! {
                                span { class: "json-lint-badge json-lint-err", title: "{e}", "⚠ Invalid JSON" }
                            },
                        }
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
                    tr {
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
    rsx! {
        select {
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
            select {
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
