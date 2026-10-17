//! gRPC unary happy path over HTTP/2.
//! Request JSON encoded as `google.protobuf.Struct` (prost-types).
//! Server must accept Struct (or compatible) for the given method — documented gap vs full protoset.

use std::time::Instant;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use prost::Message;
use prost_types::{value::Kind, ListValue, Struct, Value};
use serde_json::Value as JsonValue;
use tokio::runtime::Runtime;

#[derive(Debug, Clone)]
pub struct GrpcUnaryResult {
    pub status: String,
    pub grpc_status: i32,
    pub grpc_message: String,
    pub body_json: String,
    pub elapsed_ms: u128,
    pub error: Option<String>,
    pub log: Vec<String>,
}

/// Call unary RPC: `endpoint` like `http://127.0.0.1:50051`, method `pkg.Svc/Method`, JSON body.
pub fn grpc_unary(endpoint: &str, method: &str, json_body: &str) -> GrpcUnaryResult {
    let started = Instant::now();
    let mut log = vec!["grpc: unary (Struct JSON encode)".into()];

    let rt = match Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            return GrpcUnaryResult {
                status: "ERROR".into(),
                grpc_status: -1,
                grpc_message: String::new(),
                body_json: String::new(),
                elapsed_ms: 0,
                error: Some(format!("runtime: {e}")),
                log,
            };
        }
    };

    match rt.block_on(unary_async(endpoint, method, json_body, &mut log)) {
        Ok(mut r) => {
            r.elapsed_ms = started.elapsed().as_millis();
            r.log = log;
            r
        }
        Err(e) => GrpcUnaryResult {
            status: "ERROR".into(),
            grpc_status: -1,
            grpc_message: String::new(),
            body_json: String::new(),
            elapsed_ms: started.elapsed().as_millis(),
            error: Some(e),
            log,
        },
    }
}

async fn unary_async(
    endpoint: &str,
    method: &str,
    json_body: &str,
    log: &mut Vec<String>,
) -> Result<GrpcUnaryResult, String> {
    let endpoint = endpoint.trim_end_matches('/');
    let method = method.trim_start_matches('/');
    let url = format!("{endpoint}/{method}");
    log.push(format!("grpc: POST {url}"));

    let json: JsonValue = if json_body.trim().is_empty() {
        JsonValue::Object(serde_json::Map::new())
    } else {
        serde_json::from_str(json_body).map_err(|e| format!("json: {e}"))?
    };
    let msg = json_to_struct(&json);
    let mut buf = BytesMut::new();
    msg.encode(&mut buf)
        .map_err(|e| format!("encode: {e}"))?;
    let frame = encode_grpc_frame(&buf);
    log.push(format!("grpc: frame {} bytes", frame.len()));

    let mut connector = HttpConnector::new();
    connector.enforce_http(false);
    let client: Client<_, Full<Bytes>> =
        Client::builder(TokioExecutor::new()).build(connector);

    let req = Request::builder()
        .method(hyper::Method::POST)
        .uri(&url)
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .header("user-agent", "notPostman/0.1 grpc-unary")
        .body(Full::new(Bytes::from(frame)))
        .map_err(|e| format!("build: {e}"))?;

    let res = client.request(req).await.map_err(|e| format!("http2: {e}"))?;
    let (parts, body) = res.into_parts();
    let collected = body
        .collect()
        .await
        .map_err(|e| format!("body: {e}"))?
        .to_bytes();

    let grpc_status = parts
        .headers
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let grpc_message = parts
        .headers
        .get("grpc-message")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let body_json = match decode_grpc_message(&collected) {
        Ok(bytes) => match Struct::decode(bytes.as_ref()) {
            Ok(s) => serde_json::to_string_pretty(&struct_to_json(&s))
                .unwrap_or_else(|_| "{}".into()),
            Err(_) => {
                // Fallback: show base64 of payload for non-Struct responses.
                format!(
                    "{{\"raw_protobuf_b64\":\"{}\"}}",
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes)
                )
            }
        },
        Err(e) => format!("{{\"decode_error\":\"{e}\"}}"),
    };

    Ok(GrpcUnaryResult {
        status: if grpc_status == 0 {
            "OK".into()
        } else {
            format!("gRPC {grpc_status}")
        },
        grpc_status,
        grpc_message: grpc_message.clone(),
        body_json,
        elapsed_ms: 0,
        error: if grpc_status != 0 {
            Some(format!("grpc-status={grpc_status} {grpc_message}"))
        } else {
            None
        },
        log: vec![],
    })
}

fn encode_grpc_frame(msg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + msg.len());
    out.put_u8(0); // no compression
    out.put_u32(msg.len() as u32);
    out.extend_from_slice(msg);
    out
}

fn decode_grpc_message(frame: &[u8]) -> Result<Bytes, String> {
    if frame.is_empty() {
        return Ok(Bytes::new());
    }
    if frame.len() < 5 {
        return Err("short frame".into());
    }
    let mut cur = frame;
    let _flag = cur.get_u8();
    let len = cur.get_u32() as usize;
    if cur.remaining() < len {
        return Err("truncated payload".into());
    }
    Ok(cur.copy_to_bytes(len))
}

fn json_to_struct(v: &JsonValue) -> Struct {
    match v {
        JsonValue::Object(map) => {
            let mut fields = std::collections::BTreeMap::new();
            for (k, val) in map {
                fields.insert(k.clone(), json_to_value(val));
            }
            Struct { fields }
        }
        other => {
            let mut fields = std::collections::BTreeMap::new();
            fields.insert("value".into(), json_to_value(other));
            Struct { fields }
        }
    }
}

fn json_to_value(v: &JsonValue) -> Value {
    let kind = match v {
        JsonValue::Null => Kind::NullValue(0),
        JsonValue::Bool(b) => Kind::BoolValue(*b),
        JsonValue::Number(n) => Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
        JsonValue::String(s) => Kind::StringValue(s.clone()),
        JsonValue::Array(arr) => Kind::ListValue(ListValue {
            values: arr.iter().map(json_to_value).collect(),
        }),
        JsonValue::Object(map) => {
            let mut fields = std::collections::BTreeMap::new();
            for (k, val) in map {
                fields.insert(k.clone(), json_to_value(val));
            }
            Kind::StructValue(Struct { fields })
        }
    };
    Value { kind: Some(kind) }
}

fn struct_to_json(s: &Struct) -> JsonValue {
    let mut map = serde_json::Map::new();
    for (k, v) in &s.fields {
        map.insert(k.clone(), value_to_json(v));
    }
    JsonValue::Object(map)
}

fn value_to_json(v: &Value) -> JsonValue {
    match &v.kind {
        None => JsonValue::Null,
        Some(Kind::NullValue(_)) => JsonValue::Null,
        Some(Kind::NumberValue(n)) => serde_json::Number::from_f64(*n)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Some(Kind::StringValue(s)) => JsonValue::String(s.clone()),
        Some(Kind::BoolValue(b)) => JsonValue::Bool(*b),
        Some(Kind::StructValue(s)) => struct_to_json(s),
        Some(Kind::ListValue(l)) => JsonValue::Array(l.values.iter().map(value_to_json).collect()),
    }
}

/// Server-streaming stub: sends unary request, reads multiple gRPC frames from response body.
/// Full client/bidi streaming + reflection require `.proto` / tonic — documented gap.
pub fn grpc_server_stream_stub(
    endpoint: &str,
    method: &str,
    json_body: &str,
    max_frames: usize,
) -> GrpcUnaryResult {
    let mut result = grpc_unary(endpoint, method, json_body);
    result.log.push(format!(
        "grpc: server-stream stub (max_frames={max_frames}) — parsed first frame only; use unary for Struct APIs"
    ));
    result.log.push(
        "grpc: reflection / .proto / client+bidi streaming not wired (need tonic+prost codegen)"
            .into(),
    );
    result
}

/// List reflection services — stub message until tonic-reflection client lands.
pub fn grpc_reflection_list(endpoint: &str) -> Vec<String> {
    vec![
        format!("grpc reflection: endpoint={endpoint}"),
        "grpc reflection: not implemented — load .proto manually or use unary Struct".into(),
        "hint: set method to package.Service/Method and POST JSON as google.protobuf.Struct".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_roundtrip() {
        let payload = b"hello";
        let frame = encode_grpc_frame(payload);
        let back = decode_grpc_message(&frame).unwrap();
        assert_eq!(&back[..], payload);
    }

    #[test]
    fn struct_json_encode() {
        let j: JsonValue = serde_json::json!({"name": "ada", "n": 1});
        let s = json_to_struct(&j);
        let mut buf = BytesMut::new();
        s.encode(&mut buf).unwrap();
        assert!(!buf.is_empty());
    }
}
