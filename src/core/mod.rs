//! Domain models: requests, collections, variable scopes.
//! Inspired by postman-collection / product-docs scopes (reports/04).

#![allow(unused_imports)] // re-exports form public module surface

pub mod auth_extra;
pub mod capture_proxy;
pub mod cli_runner;
pub mod cloud_sync;
pub mod codegen;
pub mod collection;
pub mod console;
pub mod cookies;
pub mod digest;
pub mod environment;
pub mod history;
pub mod impex;
pub mod mock_server;
pub mod oauth;
pub mod perf_runner;
pub mod pkcs12_util;
pub mod protocols;
pub mod request;
pub mod sandbox;
pub mod scripts;
pub mod settings;
pub mod variables;
pub mod vault;

pub use capture_proxy::{
    interceptor_limits_blurb, start_capture_proxy, CaptureProxyHandle, CapturedRequest,
};
pub use codegen::{generate_snippet, snippet_languages};
pub use collection::{
    add_folder_under, add_request_under, collapsed_except_expanded, collapsible_tree_ids,
    demo_collections, duplicate_across, filter_tree_rows, find_request_across, flatten_collections,
    flatten_collections_filtered, folder_path_to_request, item_contains_id, move_item_to,
    purge_from_trash, remove_across, rename_across, reorder_before, reorder_within_parent,
    restore_from_trash, save_request_to_collections, sort_collections, trash_collection,
    Collection, CollectionItem, Folder, TreeRow, TreeRowKind,
};
pub use console::{push_console, ConsoleLevel, ConsoleLine, CONSOLE_MAX};
pub use cookies::{Cookie, CookieJar};
pub use digest::{build_digest_authorization, digest_uri, DigestChallenge};
pub use environment::{
    demo_environments, ensure_default_environments, export_environment_json, find_environment,
    find_environment_by_name, import_environment_json, EnvVar, Environment, DEFAULT_ENV_NAMES,
};
pub use history::{push_history, HistoryEntry, HISTORY_MAX};
pub use impex::{
    bundled_postman_collections, export_collection_v21, export_request_item, import_collection_v21,
    import_curl, import_har, import_openapi3, import_postman_export_texts, import_request_item,
    limitations_blurb, ImpexError,
};
pub use mock_server::{
    draft_mock_path, routes_from_collections, save_example_to_request, start_mock, MockHandle,
    MockRoute,
};
pub use oauth::{fetch_oauth2_token, OAuthError};
pub use perf_runner::{run_performance, PerfReport};
pub use protocols::{
    graphql_ws_subscribe, grpc_reflection_list, grpc_server_stream_stub, grpc_unary, mcp_call_tool,
    mcp_list_tools, mqtt_publish, mqtt_subscribe_once, socketio_session, split_mcp_args, sse_listen,
    ws_connect_session, GrpcUnaryResult, McpToolInfo, MqttActionResult, SseEvent, WsSession,
    WsSessionEvent,
};
pub use request::{
    AuthConfig, AuthExtra, AuthType, BodyMode, FormDataField, HttpMethod, KeyValue, OAuth2Config,
    OAuth2Grant, RequestDraft, RequestProtocol, ResponseView, SavedExample, TestResult,
    TimingBreakdown,
};
pub use sandbox::{
    apply_request_header_upserts, body_mode_label, run_js_sandbox, run_js_sandbox_ex,
    SandboxExecution, SandboxExtras, SandboxOutcome, SandboxResponse,
};
pub use scripts::{run_script, run_script_ex, run_script_helpers, ScriptRunOutcome};
pub use cloud_sync::{
    apply_sync_to_snapshot, branch_name_for_env, clone_or_ensure, git_available, link_folder,
    open_in_file_manager, pull_workspace, push_workspace, refresh_status, status_summary,
    switch_env_branch, sync_environment, sync_on_save, SyncResult,
};
pub use settings::{
    AccentIntensity, AppSettings, ClientCertSettings, CloudSyncSettings, DefaultResponseTab,
    ProxyMode, ProxySettings, RequestSettings, UiDensity, UiSettings, DEFAULT_CLOUD_REMOTE,
};
pub use variables::{
    apply_collection_map, apply_collection_var_edit, apply_env_var_edit, apply_environment_map,
    find_var_at, iter_var_spans, render_var_highlight_html, VarHoverInfo, VarResolveScope, VarSpan,
    VariableContext, VariableScope,
};
pub use vault::{resolve_vault_key, VaultStore, VAULT_ACCOUNT_ID};
