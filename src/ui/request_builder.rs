use dioxus::prelude::*;

use crate::core::{
    fetch_oauth2_token, AuthConfig, AuthType, BodyMode, FormDataField, HttpMethod, KeyValue,
    OAuth2Grant, RequestDraft, RequestProtocol, RequestSettings,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BuilderTab {
    Params,
    Headers,
    Body,
    Auth,
    Scripts,
    Settings,
}

#[component]
pub fn RequestBuilder(
    draft: RequestDraft,
    on_change: EventHandler<RequestDraft>,
    on_send: EventHandler<()>,
    /// Protocol actions: ws_connect | ws_send | ws_disconnect | mqtt_publish | mqtt_subscribe | mcp_list | mcp_call
    on_protocol_action: EventHandler<String>,
    sending: bool,
    ws_connected: bool,
    protocol_log: Vec<String>,
) -> Element {
    let mut tab = use_signal(|| BuilderTab::Params);

    let set_method = {
        let draft = draft.clone();
        let on_change = on_change.clone();
        move |evt: Event<FormData>| {
            let mut d = draft.clone();
            d.method = parse_method(&evt.value());
            on_change.call(d);
        }
    };

    let set_url = {
        let draft = draft.clone();
        let on_change = on_change.clone();
        move |evt: Event<FormData>| {
            let mut d = draft.clone();
            d.url = evt.value();
            on_change.call(d);
        }
    };

    let set_protocol = {
        let draft = draft.clone();
        let on_change = on_change.clone();
        move |evt: Event<FormData>| {
            let mut d = draft.clone();
            d.protocol = parse_protocol(&evt.value());
            on_change.call(d);
        }
    };

    rsx! {
        div { class: "request-row",
            select {
                class: "method-select protocol-select",
                value: "{draft.protocol}",
                onchange: set_protocol,
                for p in RequestProtocol::all() {
                    option { value: "{p}", selected: *p == draft.protocol, "{p}" }
                }
            }
            if draft.protocol == RequestProtocol::Http {
                select {
                    class: "method-select",
                    value: "{draft.method}",
                    onchange: set_method,
                    for m in HttpMethod::all() {
                        option { value: "{m}", selected: *m == draft.method, "{m}" }
                    }
                }
            }
            input {
                class: "url-input",
                r#type: "text",
                value: "{draft.url}",
                placeholder: match draft.protocol {
                    RequestProtocol::Http => "https://api.example.com/{{path}}",
                    RequestProtocol::WebSocket => "wss://echo.websocket.events",
                    RequestProtocol::Grpc => "http://127.0.0.1:50051",
                    RequestProtocol::Mqtt => "mqtt://127.0.0.1:1883",
                    RequestProtocol::Mcp => "(STDIO — use MCP fields below)",
                },
                oninput: set_url,
            }
            if draft.protocol == RequestProtocol::Http || draft.protocol == RequestProtocol::Grpc {
                button {
                    class: "btn btn-send",
                    disabled: sending,
                    onclick: move |_| on_send.call(()),
                    if sending { "Sending…" } else if draft.protocol == RequestProtocol::Grpc { "Invoke" } else { "Send" }
                }
            }
        }

        if draft.protocol != RequestProtocol::Http {
            ProtocolFields {
                draft: draft.clone(),
                on_change: on_change.clone(),
                on_protocol_action: on_protocol_action.clone(),
                ws_connected,
                protocol_log: protocol_log.clone(),
                sending,
            }
        }

        if draft.protocol == RequestProtocol::Http {
        div { class: "builder-tabs",
            for (label, t) in [
                ("Params", BuilderTab::Params),
                ("Headers", BuilderTab::Headers),
                ("Body", BuilderTab::Body),
                ("Auth", BuilderTab::Auth),
                ("Scripts", BuilderTab::Scripts),
                ("Settings", BuilderTab::Settings),
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
                BuilderTab::Params => rsx! {
                    KvEditor {
                        rows: draft.params.clone(),
                        on_change: {
                            let draft = draft.clone();
                            move |rows| {
                                let mut d = draft.clone();
                                d.params = rows;
                                on_change.call(d);
                            }
                        }
                    }
                },
                BuilderTab::Headers => rsx! {
                    KvEditor {
                        rows: draft.headers.clone(),
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
                        urlencoded: draft.body_urlencoded.clone(),
                        formdata: draft.body_formdata.clone(),
                        graphql_query: draft.graphql_query.clone(),
                        graphql_variables: draft.graphql_variables.clone(),
                        on_change: {
                            let draft = draft.clone();
                            move |(mode, body, urlencoded, formdata, gql_q, gql_v)| {
                                let mut d = draft.clone();
                                d.body_mode = mode;
                                d.body = body;
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
                BuilderTab::Scripts => rsx! {
                    ScriptsEditor {
                        pre: draft.pre_request_script.clone(),
                        test: draft.test_script.clone(),
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
        _ => RequestProtocol::Http,
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
                    textarea {
                        class: "body-editor",
                        value: "{draft.ws_message}",
                        rows: "3",
                        oninput: {
                            let draft = draft.clone();
                            move |evt: Event<FormData>| {
                                let mut d = draft.clone();
                                d.ws_message = evt.value();
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
                        span { class: "hint", if ws_connected { "connected" } else { "disconnected" } }
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
                    textarea {
                        class: "body-editor",
                        value: "{draft.body}",
                        rows: "6",
                        oninput: {
                            let draft = draft.clone();
                            move |evt: Event<FormData>| {
                                let mut d = draft.clone();
                                d.body = evt.value();
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
                    textarea {
                        class: "body-editor",
                        value: "{draft.body}",
                        rows: "3",
                        oninput: {
                            let draft = draft.clone();
                            move |evt: Event<FormData>| {
                                let mut d = draft.clone();
                                d.body = evt.value();
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
                    textarea {
                        class: "body-editor",
                        value: "{draft.mcp_tool_args_json}",
                        rows: "4",
                        oninput: {
                            let draft = draft.clone();
                            move |evt: Event<FormData>| {
                                let mut d = draft.clone();
                                d.mcp_tool_args_json = evt.value();
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

#[component]
fn KvEditor(rows: Vec<KeyValue>, on_change: EventHandler<Vec<KeyValue>>) -> Element {
    rsx! {
        table { class: "kv-table",
            thead {
                tr {
                    th { "Key" }
                    th { "Value" }
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
                                        ensure_trailing_empty(&mut next);
                                        on_change.call(next);
                                    }
                                }
                            }
                        }
                        td {
                            input {
                                r#type: "text",
                                value: "{row.value}",
                                oninput: {
                                    let rows = rows.clone();
                                    move |evt: Event<FormData>| {
                                        let mut next = rows.clone();
                                        if let Some(r) = next.get_mut(i) {
                                            r.value = evt.value();
                                        }
                                        ensure_trailing_empty(&mut next);
                                        on_change.call(next);
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

#[component]
fn BodyEditor(
    mode: BodyMode,
    body: String,
    urlencoded: Vec<KeyValue>,
    formdata: Vec<FormDataField>,
    graphql_query: String,
    graphql_variables: String,
    on_change: EventHandler<(
        BodyMode,
        String,
        Vec<KeyValue>,
        Vec<FormDataField>,
        String,
        String,
    )>,
) -> Element {
    let mode_str = match mode {
        BodyMode::None => "none",
        BodyMode::Raw => "raw",
        BodyMode::Urlencoded => "urlencoded",
        BodyMode::FormData => "formdata",
        BodyMode::Graphql => "graphql",
    };
    rsx! {
        select {
            value: "{mode_str}",
            onchange: {
                let body = body.clone();
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
                        _ => BodyMode::None,
                    };
                    on_change.call((
                        mode,
                        body.clone(),
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
        }
        if mode == BodyMode::Raw {
            textarea {
                class: "body-editor",
                value: "{body}",
                placeholder: "{{\"hello\": \"world\"}}",
                oninput: {
                    let urlencoded = urlencoded.clone();
                    let formdata = formdata.clone();
                    let graphql_query = graphql_query.clone();
                    let graphql_variables = graphql_variables.clone();
                    move |evt| on_change.call((
                        mode,
                        evt.value(),
                        urlencoded.clone(),
                        formdata.clone(),
                        graphql_query.clone(),
                        graphql_variables.clone(),
                    ))
                },
            }
        } else if mode == BodyMode::Urlencoded {
            KvEditor {
                rows: urlencoded.clone(),
                on_change: {
                    let body = body.clone();
                    let formdata = formdata.clone();
                    let graphql_query = graphql_query.clone();
                    let graphql_variables = graphql_variables.clone();
                    move |rows| on_change.call((
                        BodyMode::Urlencoded,
                        body.clone(),
                        rows,
                        formdata.clone(),
                        graphql_query.clone(),
                        graphql_variables.clone(),
                    ))
                }
            }
        } else if mode == BodyMode::FormData {
            p { class: "hint", "Check File? and put a local path in Value to upload that file." }
            FormDataEditor {
                rows: formdata.clone(),
                on_change: {
                    let body = body.clone();
                    let urlencoded = urlencoded.clone();
                    let graphql_query = graphql_query.clone();
                    let graphql_variables = graphql_variables.clone();
                    move |rows| on_change.call((
                        BodyMode::FormData,
                        body.clone(),
                        urlencoded.clone(),
                        rows,
                        graphql_query.clone(),
                        graphql_variables.clone(),
                    ))
                }
            }
        } else if mode == BodyMode::Graphql {
            label { class: "hint", "Query" }
            textarea {
                class: "body-editor",
                value: "{graphql_query}",
                placeholder: "query {{ viewer {{ login }} }}",
                oninput: {
                    let body = body.clone();
                    let urlencoded = urlencoded.clone();
                    let formdata = formdata.clone();
                    let graphql_variables = graphql_variables.clone();
                    move |evt| on_change.call((
                        BodyMode::Graphql,
                        body.clone(),
                        urlencoded.clone(),
                        formdata.clone(),
                        evt.value(),
                        graphql_variables.clone(),
                    ))
                },
            }
            label { class: "hint", "Variables (JSON)" }
            textarea {
                class: "body-editor",
                value: "{graphql_variables}",
                placeholder: "{{}}",
                oninput: {
                    let body = body.clone();
                    let urlencoded = urlencoded.clone();
                    let formdata = formdata.clone();
                    let graphql_query = graphql_query.clone();
                    move |evt| on_change.call((
                        BodyMode::Graphql,
                        body.clone(),
                        urlencoded.clone(),
                        formdata.clone(),
                        graphql_query.clone(),
                        evt.value(),
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
    on_change: EventHandler<Vec<FormDataField>>,
) -> Element {
    rsx! {
        table { class: "kv-table",
            thead {
                tr {
                    th { "Key" }
                    th { "Value" }
                    th { "File?" }
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
                            input {
                                r#type: "text",
                                value: "{row.value}",
                                placeholder: if row.is_file { "/path/to/file" } else { "value" },
                                oninput: {
                                    let rows = rows.clone();
                                    move |evt: Event<FormData>| {
                                        let mut next = rows.clone();
                                        if let Some(r) = next.get_mut(i) {
                                            r.value = evt.value();
                                        }
                                        ensure_trailing_form(&mut next);
                                        on_change.call(next);
                                    }
                                }
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
fn AuthEditor(auth: AuthConfig, on_change: EventHandler<AuthConfig>) -> Element {
    let auth_type = auth.auth_type;
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
        }
        if matches!(auth_type, AuthType::Bearer | AuthType::ApiKey) {
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
        if auth_type == AuthType::ApiKey {
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
        if matches!(auth_type, AuthType::Basic | AuthType::Digest) {
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
                    let mut a = auth.clone();
                    move |_| {
                        match fetch_oauth2_token(&mut a.oauth2) {
                            Ok(()) => on_change.call(a.clone()),
                            Err(e) => {
                                a.oauth2.access_token = format!("ERROR: {e}");
                                on_change.call(a.clone());
                            }
                        }
                    }
                },
                "Get New Access Token"
            }
        }
        if auth_type == AuthType::Inherit {
            p { class: "hint", "Uses collection-level auth when set." }
        }
        if auth_type == AuthType::Digest {
            p { class: "hint", "Digest: MD5 / qop=auth challenge after 401." }
        }
        p { class: "hint", "secret://… → keychain · {{{{vault:key}}}} → vault.json / keychain vault." }
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
    }
}

fn parse_auth(s: &str) -> AuthType {
    match s {
        "bearer" => AuthType::Bearer,
        "basic" => AuthType::Basic,
        "api_key" => AuthType::ApiKey,
        "digest" => AuthType::Digest,
        "oauth2" => AuthType::Oauth2,
        "inherit" => AuthType::Inherit,
        _ => AuthType::NoAuth,
    }
}

#[component]
fn ScriptsEditor(
    pre: String,
    test: String,
    on_change: EventHandler<(String, String)>,
) -> Element {
    rsx! {
        p { class: "hint",
            "JS sandbox: pm.variables|environment|collectionVariables|globals|vault|cookies|request|execution|iterationData|sendRequest, pm.response, pm.test/expect. Unsupported: async/require."
        }
        label { "Pre-request" }
        textarea {
            class: "body-editor",
            value: "{pre}",
            placeholder: "pm.environment.set(\"baseUrl\", \"https://httpbin.org\");\nconsole.log(\"pre\");",
            oninput: {
                let test = test.clone();
                move |evt| on_change.call((evt.value(), test.clone()))
            }
        }
        label { "Tests (post-response)" }
        textarea {
            class: "body-editor",
            value: "{test}",
            placeholder: "pm.test(\"status ok\", function () {{\n  pm.expect(pm.response.code).to.equal(200);\n}});",
            oninput: {
                let pre = pre.clone();
                move |evt| on_change.call((pre.clone(), evt.value()))
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
