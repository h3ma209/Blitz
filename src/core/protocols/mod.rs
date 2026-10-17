//! Local multi-protocol clients (WS / gRPC unary / MQTT / MCP STDIO).
//! Inspired by ASAR `@postman/runtime_*-client` presence — not a Cloud clone.

pub mod grpc;
pub mod mcp;
pub mod mqtt;
pub mod websocket;

pub use grpc::{grpc_unary, GrpcUnaryResult};
pub use mcp::{mcp_call_tool, mcp_list_tools, split_mcp_args, McpToolInfo};
pub use mqtt::{mqtt_publish, mqtt_subscribe_once, MqttActionResult};
pub use websocket::{ws_connect_session, WsSession, WsSessionEvent};
