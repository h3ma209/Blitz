//! MQTT connect / publish / subscribe-once via rumqttc.

use std::time::{Duration, Instant};

use rumqttc::{Client, Event, Incoming, MqttOptions, QoS};

#[derive(Debug, Clone)]
pub struct MqttActionResult {
    pub ok: bool,
    pub messages: Vec<String>,
    pub error: Option<String>,
    pub elapsed_ms: u128,
}

/// Publish one message then disconnect.
pub fn mqtt_publish(
    broker_url: &str,
    client_id: &str,
    topic: &str,
    payload: &str,
) -> MqttActionResult {
    let started = Instant::now();
    match with_client(broker_url, client_id, |client, connection| {
        client
            .publish(topic, QoS::AtLeastOnce, false, payload.as_bytes())
            .map_err(|e| e.to_string())?;
        // Drain a few events so publish is flushed.
        for _ in 0..20 {
            match connection.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(Event::Incoming(Incoming::PubAck(_)))) => break,
                Ok(Ok(_)) => {}
                Ok(Err(e)) => return Err(e.to_string()),
                Err(_) => break,
            }
        }
        Ok(vec![format!("published → {topic}: {payload}")])
    }) {
        Ok(messages) => MqttActionResult {
            ok: true,
            messages,
            error: None,
            elapsed_ms: started.elapsed().as_millis(),
        },
        Err(e) => MqttActionResult {
            ok: false,
            messages: vec![],
            error: Some(e),
            elapsed_ms: started.elapsed().as_millis(),
        },
    }
}

/// Subscribe and wait for one message (or timeout).
pub fn mqtt_subscribe_once(
    broker_url: &str,
    client_id: &str,
    topic: &str,
    timeout: Duration,
) -> MqttActionResult {
    let started = Instant::now();
    match with_client(broker_url, client_id, |client, connection| {
        client
            .subscribe(topic, QoS::AtLeastOnce)
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + timeout;
        let mut out = vec![format!("subscribed {topic}")];
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now());
            match connection.recv_timeout(remain.min(Duration::from_millis(250))) {
                Ok(Ok(Event::Incoming(Incoming::Publish(p)))) => {
                    let body = String::from_utf8_lossy(&p.payload).to_string();
                    out.push(format!("← {}: {}", p.topic, body));
                    return Ok(out);
                }
                Ok(Ok(_)) => {}
                Ok(Err(e)) => return Err(e.to_string()),
                Err(_) => {}
            }
        }
        out.push("timeout waiting for message".into());
        Ok(out)
    }) {
        Ok(messages) => MqttActionResult {
            ok: true,
            messages,
            error: None,
            elapsed_ms: started.elapsed().as_millis(),
        },
        Err(e) => MqttActionResult {
            ok: false,
            messages: vec![],
            error: Some(e),
            elapsed_ms: started.elapsed().as_millis(),
        },
    }
}

fn with_client<F>(broker_url: &str, client_id: &str, f: F) -> Result<Vec<String>, String>
where
    F: FnOnce(&Client, &mut rumqttc::Connection) -> Result<Vec<String>, String>,
{
    let (host, port) = parse_broker(broker_url)?;
    let mut opts = MqttOptions::new(client_id, host, port);
    opts.set_keep_alive(Duration::from_secs(5));
    let (client, mut connection) = Client::new(opts, 10);
    let result = f(&client, &mut connection);
    let _ = client.disconnect();
    // Drain briefly
    for _ in 0..5 {
        if connection.recv_timeout(Duration::from_millis(50)).is_err() {
            break;
        }
    }
    result
}

fn parse_broker(url: &str) -> Result<(String, u16), String> {
    let u = url.trim();
    let u = u
        .strip_prefix("mqtt://")
        .or_else(|| u.strip_prefix("tcp://"))
        .unwrap_or(u);
    if let Some((h, p)) = u.split_once(':') {
        let port: u16 = p
            .split('/')
            .next()
            .unwrap_or(p)
            .parse()
            .map_err(|e| format!("port: {e}"))?;
        Ok((h.to_string(), port))
    } else {
        Ok((u.split('/').next().unwrap_or(u).to_string(), 1883))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_broker_defaults() {
        assert_eq!(
            parse_broker("mqtt://localhost:1883").unwrap(),
            ("localhost".into(), 1883)
        );
        assert_eq!(parse_broker("127.0.0.1").unwrap(), ("127.0.0.1".into(), 1883));
    }
}
