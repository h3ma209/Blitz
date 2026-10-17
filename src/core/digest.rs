//! HTTP Digest authentication (RFC 2617 MD5 / auth qop).

use md5::{Digest, Md5};

/// Parsed `WWW-Authenticate: Digest …` challenge.
#[derive(Debug, Clone, Default)]
pub struct DigestChallenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    pub qop: Option<String>,
    pub algorithm: String,
}

impl DigestChallenge {
    /// Parse first Digest challenge from a WWW-Authenticate header value.
    pub fn parse(header: &str) -> Option<Self> {
        let trimmed = header.trim();
        let rest = trimmed.strip_prefix("Digest ").or_else(|| {
            // Some servers send multiple schemes; find Digest=
            trimmed
                .find("Digest ")
                .map(|i| trimmed[i + "Digest ".len()..].trim())
        })?;

        let mut out = Self {
            algorithm: "MD5".into(),
            ..Default::default()
        };

        for part in split_auth_params(rest) {
            let (k, v) = match part.split_once('=') {
                Some(kv) => kv,
                None => continue,
            };
            let key = k.trim().to_ascii_lowercase();
            let val = unquote(v.trim());
            match key.as_str() {
                "realm" => out.realm = val,
                "nonce" => out.nonce = val,
                "opaque" => out.opaque = Some(val),
                "qop" => out.qop = Some(val.split(',').next().unwrap_or(&val).trim().into()),
                "algorithm" => out.algorithm = val,
                _ => {}
            }
        }

        if out.realm.is_empty() || out.nonce.is_empty() {
            return None;
        }
        Some(out)
    }
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s[1..s.len() - 1].replace("\\\"", "\"")
    } else {
        s.to_string()
    }
}

fn split_auth_params(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for ch in s.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(ch);
            }
            ',' if !in_quotes => {
                if !cur.trim().is_empty() {
                    parts.push(cur.trim().to_string());
                }
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur.trim().to_string());
    }
    parts
}

fn md5_hex(data: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(data.as_bytes());
    hex::encode(hasher.finalize())
}

/// Build `Authorization: Digest …` header value.
pub fn build_digest_authorization(
    challenge: &DigestChallenge,
    username: &str,
    password: &str,
    method: &str,
    uri: &str,
    nc: u32,
    cnonce: &str,
) -> Result<String, String> {
    let algo = challenge.algorithm.to_ascii_uppercase();
    if algo != "MD5" && algo != "MD5-SESS" {
        return Err(format!(
            "digest: unsupported algorithm {algo} (only MD5 / MD5-SESS)"
        ));
    }

    let ha1_base = md5_hex(&format!(
        "{username}:{}:{password}",
        challenge.realm
    ));
    let ha1 = if algo == "MD5-SESS" {
        md5_hex(&format!("{ha1_base}:{}:{cnonce}", challenge.nonce))
    } else {
        ha1_base
    };

    let ha2 = md5_hex(&format!("{method}:{uri}"));
    let nc_str = format!("{nc:08x}");

    let response = if let Some(qop) = &challenge.qop {
        let qop = if qop.contains("auth") { "auth" } else { qop.as_str() };
        if qop != "auth" {
            return Err(format!("digest: unsupported qop {qop}"));
        }
        md5_hex(&format!(
            "{ha1}:{}:{nc_str}:{cnonce}:{qop}:{ha2}",
            challenge.nonce
        ))
    } else {
        md5_hex(&format!("{ha1}:{}:{ha2}", challenge.nonce))
    };

    let mut parts = vec![
        format!("username=\"{username}\""),
        format!("realm=\"{}\"", challenge.realm),
        format!("nonce=\"{}\"", challenge.nonce),
        format!("uri=\"{uri}\""),
        format!("response=\"{response}\""),
    ];
    if let Some(qop) = &challenge.qop {
        let qop = if qop.contains("auth") { "auth" } else { qop.as_str() };
        parts.push(format!("qop={qop}"));
        parts.push(format!("nc={nc_str}"));
        parts.push(format!("cnonce=\"{cnonce}\""));
    }
    if let Some(opaque) = &challenge.opaque {
        parts.push(format!("opaque=\"{opaque}\""));
    }
    if !challenge.algorithm.is_empty() {
        parts.push(format!("algorithm={}", challenge.algorithm));
    }

    Ok(format!("Digest {}", parts.join(", ")))
}

/// Extract request-target path+query for Digest uri= parameter.
pub fn digest_uri(url: &str) -> String {
    if let Ok(u) = url::Url::parse(url) {
        let mut s = u.path().to_string();
        if let Some(q) = u.query() {
            s.push('?');
            s.push_str(q);
        }
        if s.is_empty() {
            "/".into()
        } else {
            s
        }
    } else {
        // Fallback: path after host
        url.split("://")
            .nth(1)
            .and_then(|rest| rest.find('/').map(|i| rest[i..].to_string()))
            .unwrap_or_else(|| "/".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_challenge() {
        let h = r#"Digest realm="testrealm@host.com", qop="auth", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#;
        let c = DigestChallenge::parse(h).unwrap();
        assert_eq!(c.realm, "testrealm@host.com");
        assert_eq!(c.qop.as_deref(), Some("auth"));
        assert!(!c.nonce.is_empty());
    }

    #[test]
    fn rfc2617_example_response() {
        // Adapted from RFC 2617 example values
        let challenge = DigestChallenge {
            realm: "testrealm@host.com".into(),
            nonce: "dcd98b7102dd2f0e8b11d0f600bfb0c093".into(),
            opaque: Some("5ccc069c403ebaf9f0171e9517f40e41".into()),
            qop: Some("auth".into()),
            algorithm: "MD5".into(),
        };
        let auth = build_digest_authorization(
            &challenge,
            "Mufasa",
            "Circle Of Life",
            "GET",
            "/dir/index.html",
            1,
            "0a4f113b",
        )
        .unwrap();
        assert!(auth.contains("response=\"6629fae49393a05397450978507c4ef1\""));
    }
}
