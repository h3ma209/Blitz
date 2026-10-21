//! Extra auth signing helpers: OAuth1, Hawk, AWS SigV4, EdgeGrid, NTLM (Type1 stub).
//! Partial parity with postman-runtime auth kinds — see FEATURES.md.

use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use super::request::{AuthConfig, AuthType, KeyValue, RequestDraft};

type HmacSha1 = Hmac<Sha1>;
type HmacSha256 = Hmac<Sha256>;

/// Inject Authorization (and related) headers for advanced auth types.
pub fn inject_extra_auth(draft: &mut RequestDraft, log: &mut Vec<String>) {
    match draft.auth.auth_type {
        AuthType::Oauth1 => inject_oauth1(draft, log),
        AuthType::Hawk => inject_hawk(draft, log),
        AuthType::AwsSigV4 => inject_aws_sigv4(draft, log),
        AuthType::EdgeGrid => inject_edgegrid(draft, log),
        AuthType::Ntlm => {
            // Type1 negotiate header — full challenge/response needs connection reuse.
            let type1 = ntlm_type1_message(&draft.auth.username);
            draft.headers.push(KeyValue::new(
                "Authorization",
                format!("NTLM {type1}"),
            ));
            log.push(
                "auth: NTLM Type1 negotiate injected (full handshake needs keep-alive 401 loop — partial)"
                    .into(),
            );
        }
        _ => {}
    }
}

fn inject_oauth1(draft: &mut RequestDraft, log: &mut Vec<String>) {
    let consumer_key = draft.auth.extra.key.clone();
    let consumer_secret = draft.auth.extra.secret.clone();
    let token = draft.auth.extra.token.clone();
    let token_secret = draft.auth.extra.token_secret.clone();
    let method = if draft.auth.extra.signature_method.is_empty() {
        "HMAC-SHA1".to_string()
    } else {
        draft.auth.extra.signature_method.clone()
    };

    if consumer_key.is_empty() {
        log.push("auth: oauth1 skipped (empty consumer key)".into());
        return;
    }

    let nonce = format!("{:x}", uuid::Uuid::new_v4().as_u128());
    let timestamp = chrono::Utc::now().timestamp().to_string();
    let url = draft.url.split('?').next().unwrap_or(&draft.url);

    let mut params: Vec<(String, String)> = vec![
        ("oauth_consumer_key".into(), consumer_key.clone()),
        ("oauth_nonce".into(), nonce.clone()),
        ("oauth_signature_method".into(), method.clone()),
        ("oauth_timestamp".into(), timestamp.clone()),
        ("oauth_version".into(), "1.0".into()),
    ];
    if !token.is_empty() {
        params.push(("oauth_token".into(), token.clone()));
    }
    // Include query params from URL for base string
    if let Some(q) = draft.url.split_once('?').map(|(_, q)| q) {
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                params.push((pct_decode_lite(k), pct_decode_lite(v)));
            }
        }
    }
    params.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let param_str = params
        .iter()
        .map(|(k, v)| format!("{}={}", pct_encode(k), pct_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let base = format!(
        "{}&{}&{}",
        draft.method.as_str(),
        pct_encode(url),
        pct_encode(&param_str)
    );
    let key = format!(
        "{}&{}",
        pct_encode(&consumer_secret),
        pct_encode(&token_secret)
    );

    let signature = match method.to_uppercase().as_str() {
        "PLAINTEXT" => key.clone(),
        "HMAC-SHA256" => {
            let mut mac = HmacSha256::new_from_slice(key.as_bytes()).unwrap();
            mac.update(base.as_bytes());
            base64_encode(&mac.finalize().into_bytes())
        }
        _ => {
            let mut mac = HmacSha1::new_from_slice(key.as_bytes()).unwrap();
            mac.update(base.as_bytes());
            base64_encode(&mac.finalize().into_bytes())
        }
    };

    let mut auth_params = vec![
        ("oauth_consumer_key", consumer_key),
        ("oauth_nonce", nonce),
        ("oauth_signature", signature),
        ("oauth_signature_method", method),
        ("oauth_timestamp", timestamp),
        ("oauth_version", "1.0".into()),
    ];
    if !token.is_empty() {
        auth_params.push(("oauth_token", token));
    }
    let header = format!(
        "OAuth {}",
        auth_params
            .iter()
            .map(|(k, v)| format!("{k}=\"{}\"", pct_encode(v)))
            .collect::<Vec<_>>()
            .join(", ")
    );
    draft
        .headers
        .push(KeyValue::new("Authorization", header));
    log.push("auth: oauth1 Authorization injected".into());
}

fn inject_hawk(draft: &mut RequestDraft, log: &mut Vec<String>) {
    let id = draft.auth.extra.key.clone();
    let key = draft.auth.extra.secret.clone();
    if id.is_empty() || key.is_empty() {
        log.push("auth: hawk skipped (need id + key)".into());
        return;
    }
    let algo = if draft.auth.extra.algorithm.is_empty() {
        "sha256"
    } else {
        draft.auth.extra.algorithm.as_str()
    };
    let ts = chrono::Utc::now().timestamp().to_string();
    let nonce = format!("{:08x}", uuid::Uuid::new_v4().as_u128() as u32);
    let (host, port, path) = hawk_parts(&draft.url);
    let ext = draft.auth.extra.ext.clone();

    // Hawk MAC: header newline-form
    let normalized = format!(
        "hawk.1.header\n{ts}\n{nonce}\n{}\n{path}\n{host}\n{port}\n\n{ext}\n",
        draft.method.as_str().to_uppercase()
    );
    let mac = match algo {
        "sha1" => {
            let mut m = HmacSha1::new_from_slice(key.as_bytes()).unwrap();
            m.update(normalized.as_bytes());
            base64_encode(&m.finalize().into_bytes())
        }
        _ => {
            let mut m = HmacSha256::new_from_slice(key.as_bytes()).unwrap();
            m.update(normalized.as_bytes());
            base64_encode(&m.finalize().into_bytes())
        }
    };
    let mut hdr = format!("Hawk id=\"{id}\", ts=\"{ts}\", nonce=\"{nonce}\", mac=\"{mac}\"");
    if !ext.is_empty() {
        hdr.push_str(&format!(", ext=\"{ext}\""));
    }
    draft
        .headers
        .push(KeyValue::new("Authorization", hdr));
    log.push(format!("auth: hawk ({algo}) injected"));
}

fn inject_aws_sigv4(draft: &mut RequestDraft, log: &mut Vec<String>) {
    let access_key = draft.auth.extra.key.clone();
    let secret_key = draft.auth.extra.secret.clone();
    if access_key.is_empty() || secret_key.is_empty() {
        log.push("auth: aws sigv4 skipped (need access + secret key)".into());
        return;
    }
    let region = if draft.auth.extra.region.is_empty() {
        "us-east-1"
    } else {
        draft.auth.extra.region.as_str()
    };
    let service = if draft.auth.extra.service.is_empty() {
        "execute-api"
    } else {
        draft.auth.extra.service.as_str()
    };
    let session = draft.auth.extra.token.clone();

    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();

    let (host, path, query) = aws_url_parts(&draft.url);
    let payload = match draft.body_mode {
        super::request::BodyMode::None => Vec::new(),
        super::request::BodyMode::Binary => Vec::new(), // filled later; sign empty here if path body
        _ => draft.body.as_bytes().to_vec(),
    };
    let payload_hash = hex::encode(Sha256::digest(&payload));

    if !draft
        .headers
        .iter()
        .any(|h| h.key.eq_ignore_ascii_case("x-amz-date"))
    {
        draft
            .headers
            .push(KeyValue::new("x-amz-date", amz_date.clone()));
    }
    if !draft
        .headers
        .iter()
        .any(|h| h.key.eq_ignore_ascii_case("host"))
    {
        draft.headers.push(KeyValue::new("Host", host.clone()));
    }
    if !session.is_empty() {
        draft
            .headers
            .push(KeyValue::new("x-amz-security-token", session.clone()));
    }

    let mut signed_headers_list: Vec<String> = draft
        .headers
        .iter()
        .filter(|h| h.enabled && !h.key.is_empty())
        .map(|h| h.key.to_ascii_lowercase())
        .collect();
    signed_headers_list.push("host".into());
    signed_headers_list.sort();
    signed_headers_list.dedup();
    let signed_headers = signed_headers_list.join(";");

    let mut canonical_headers = String::new();
    for name in &signed_headers_list {
        let val = if name == "host" {
            host.clone()
        } else {
            draft
                .headers
                .iter()
                .find(|h| h.key.eq_ignore_ascii_case(name))
                .map(|h| h.value.trim().to_string())
                .unwrap_or_default()
        };
        canonical_headers.push_str(&format!("{name}:{val}\n"));
    }

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        draft.method.as_str(),
        if path.is_empty() { "/" } else { &path },
        query,
        canonical_headers,
        signed_headers,
        payload_hash
    );
    let canonical_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
    let credential_scope = format!("{date_stamp}/{region}/{service}/aws4_request");
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{canonical_hash}");

    let signing_key = aws_signing_key(&secret_key, &date_stamp, region, service);
    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}"
    );
    draft
        .headers
        .push(KeyValue::new("Authorization", auth));
    log.push(format!("auth: aws sigv4 ({service}/{region}) injected"));
}

fn inject_edgegrid(draft: &mut RequestDraft, log: &mut Vec<String>) {
    let client_token = draft.auth.extra.key.clone();
    let client_secret = draft.auth.extra.secret.clone();
    let access_token = draft.auth.extra.token.clone();
    let host = if !draft.auth.extra.token_secret.is_empty() {
        draft.auth.extra.token_secret.clone()
    } else {
        draft
            .url
            .split("://")
            .nth(1)
            .unwrap_or("")
            .split('/')
            .next()
            .unwrap_or("")
            .to_string()
    };
    if client_token.is_empty() || client_secret.is_empty() || access_token.is_empty() {
        log.push("auth: edgegrid skipped (need client_token, client_secret, access_token)".into());
        return;
    }
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H:%M:%S+0000").to_string();
    let nonce = uuid::Uuid::new_v4().to_string();
    let path_query = {
        let rest = draft.url.split("://").nth(1).unwrap_or(&draft.url);
        rest.find('/')
            .map(|i| &rest[i..])
            .unwrap_or("/")
            .to_string()
    };
    // EdgeGrid data-to-sign (simplified EG1)
    let body_hash = {
        let b = draft.body.as_bytes();
        if b.is_empty() {
            String::new()
        } else {
            let mut hasher = Sha256::new();
            hasher.update(b);
            base64_encode(&hasher.finalize())
        }
    };
    let data_to_sign = format!(
        "EG1-HMAC-SHA256 client_token={client_token};access_token={access_token};timestamp={timestamp};nonce={nonce};\t{}\t{host}\t{path_query}\t\t{body_hash}\t"
        ,
        draft.method.as_str()
    );
    // Signing key = HMAC(client_secret, timestamp)
    let mut sk = HmacSha256::new_from_slice(client_secret.as_bytes()).unwrap();
    sk.update(timestamp.as_bytes());
    let signing_key = sk.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(data_to_sign.as_bytes());
    let signature = base64_encode(&mac.finalize().into_bytes());
    let hdr = format!(
        "EG1-HMAC-SHA256 client_token={client_token};access_token={access_token};timestamp={timestamp};nonce={nonce};signature={signature}"
    );
    draft
        .headers
        .push(KeyValue::new("Authorization", hdr));
    log.push("auth: edgegrid EG1 injected".into());
}

fn aws_signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = {
        let mut m = HmacSha256::new_from_slice(format!("AWS4{secret}").as_bytes()).unwrap();
        m.update(date.as_bytes());
        m.finalize().into_bytes()
    };
    let k_region = {
        let mut m = HmacSha256::new_from_slice(&k_date).unwrap();
        m.update(region.as_bytes());
        m.finalize().into_bytes()
    };
    let k_service = {
        let mut m = HmacSha256::new_from_slice(&k_region).unwrap();
        m.update(service.as_bytes());
        m.finalize().into_bytes()
    };
    let mut m = HmacSha256::new_from_slice(&k_service).unwrap();
    m.update(b"aws4_request");
    m.finalize().into_bytes().to_vec()
}

fn aws_url_parts(url: &str) -> (String, String, String) {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let (host_port, path_q) = rest
        .split_once('/')
        .map(|(h, p)| (h, format!("/{p}")))
        .unwrap_or((rest, "/".into()));
    let host = host_port.to_string();
    let (path, query) = path_q
        .split_once('?')
        .map(|(p, q)| (p.to_string(), q.to_string()))
        .unwrap_or((path_q, String::new()));
    (host, path, query)
}

fn hawk_parts(url: &str) -> (String, String, String) {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let scheme_https = url.starts_with("https");
    let (host_port, path) = rest
        .split_once('/')
        .map(|(h, p)| (h, format!("/{p}")))
        .unwrap_or((rest, "/".into()));
    let path = path.split('?').next().unwrap_or(&path).to_string();
    if let Some((h, p)) = host_port.split_once(':') {
        (h.to_string(), p.to_string(), path)
    } else {
        (
            host_port.to_string(),
            if scheme_https { "443" } else { "80" }.into(),
            path,
        )
    }
}

fn ntlm_type1_message(domain_user: &str) -> String {
    // Minimal Type 1 NTLMSSP negotiate (flags OEM + NTLM + Unicode).
    let mut msg = Vec::new();
    msg.extend_from_slice(b"NTLMSSP\0");
    msg.extend_from_slice(&1u32.to_le_bytes()); // type 1
    msg.extend_from_slice(&0x0008_8207u32.to_le_bytes()); // flags
    // domain / workstation empty buffers
    for _ in 0..4 {
        msg.extend_from_slice(&0u16.to_le_bytes()); // len
        msg.extend_from_slice(&0u16.to_le_bytes()); // maxlen
        msg.extend_from_slice(&0u32.to_le_bytes()); // offset
    }
    let _ = domain_user; // username used in Type3 only
    base64_encode(&msg)
}

fn pct_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn pct_decode_lite(s: &str) -> String {
    urlencoding::decode(s)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| s.to_string())
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Human labels for AuthConfig (pipeline logs).
pub fn auth_type_label(t: AuthType) -> &'static str {
    match t {
        AuthType::NoAuth => "noauth",
        AuthType::Inherit => "inherit",
        AuthType::Bearer => "bearer",
        AuthType::Basic => "basic",
        AuthType::ApiKey => "apikey",
        AuthType::Digest => "digest",
        AuthType::Oauth2 => "oauth2",
        AuthType::Oauth1 => "oauth1",
        AuthType::Hawk => "hawk",
        AuthType::Ntlm => "ntlm",
        AuthType::AwsSigV4 => "aws_sigv4",
        AuthType::EdgeGrid => "edgegrid",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::request::AuthExtra;

    #[test]
    fn oauth1_sets_authorization() {
        let mut d = RequestDraft {
            url: "https://api.example.com/resource".into(),
            ..Default::default()
        };
        d.auth.auth_type = AuthType::Oauth1;
        d.auth.extra = AuthExtra {
            key: "ck".into(),
            secret: "cs".into(),
            token: "tk".into(),
            token_secret: "ts".into(),
            signature_method: "HMAC-SHA1".into(),
            ..Default::default()
        };
        let mut log = Vec::new();
        inject_extra_auth(&mut d, &mut log);
        assert!(d
            .headers
            .iter()
            .any(|h| h.key == "Authorization" && h.value.starts_with("OAuth ")));
    }

    #[test]
    fn hawk_sets_authorization() {
        let mut d = RequestDraft {
            url: "https://example.com/resource".into(),
            ..Default::default()
        };
        d.auth.auth_type = AuthType::Hawk;
        d.auth.extra.key = "dh37fgj492je".into();
        d.auth.extra.secret = "werxhqb98rpaxn39848xrunpaw3489ruxnpa98w4rxn".into();
        let mut log = Vec::new();
        inject_extra_auth(&mut d, &mut log);
        assert!(d
            .headers
            .iter()
            .any(|h| h.value.starts_with("Hawk ")));
    }

    #[test]
    fn aws_sigv4_sets_authorization() {
        let mut d = RequestDraft {
            method: crate::core::request::HttpMethod::Get,
            url: "https://service.region.amazonaws.com/path".into(),
            ..Default::default()
        };
        d.auth.auth_type = AuthType::AwsSigV4;
        d.auth.extra.key = "AKID".into();
        d.auth.extra.secret = "secret".into();
        d.auth.extra.region = "us-east-1".into();
        d.auth.extra.service = "service".into();
        let mut log = Vec::new();
        inject_extra_auth(&mut d, &mut log);
        assert!(d
            .headers
            .iter()
            .any(|h| h.value.starts_with("AWS4-HMAC-SHA256")));
    }
}
