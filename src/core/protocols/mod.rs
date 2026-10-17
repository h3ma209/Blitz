//! Local multi-protocol clients (WS / gRPC unary / MQTT / MCP / Socket.IO / SSE / GraphQL-WS).
//! Inspired by ASAR `@postman/runtime_*-client` presence — not a Cloud clone.

pub mod graphql_ws;
pub mod grpc;
pub mod mcp;
pub mod mqtt;
pub mod socketio;
pub mod sse;
pub mod websocket;

pub use graphql_ws::graphql_ws_subscribe;
pub use grpc::{grpc_reflection_list, grpc_server_stream_stub, grpc_unary, GrpcUnaryResult};
pub use mcp::{mcp_call_tool, mcp_list_tools, split_mcp_args, McpToolInfo};
pub use mqtt::{mqtt_publish, mqtt_subscribe_once, MqttActionResult};
pub use socketio::socketio_session;
pub use sse::{sse_listen, SseEvent};
pub use websocket::{ws_connect_session, WsSession, WsSessionEvent};
