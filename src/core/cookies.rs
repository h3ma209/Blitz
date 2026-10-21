//! Basic domain cookie jar (optional / lightweight).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Cookie {
    pub domain: String,
    pub name: String,
    pub value: String,
    #[serde(default = "default_path")]
    pub path: String,
}

fn default_path() -> String {
    "/".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CookieJar {
    pub cookies: Vec<Cookie>,
}

impl CookieJar {
    /// Store / replace cookies from `Set-Cookie` header values for a request host.
    pub fn store_from_set_cookie(&mut self, host: &str, set_cookie_values: &[String]) {
        let domain = host_to_domain(host);
        for raw in set_cookie_values {
            if let Some((name, value, path)) = parse_set_cookie(raw) {
                self.upsert(Cookie {
                    domain: domain.clone(),
                    name,
                    value,
                    path,
                });
            }
        }
    }

    pub fn upsert(&mut self, cookie: Cookie) {
        if let Some(existing) = self.cookies.iter_mut().find(|c| {
            c.domain == cookie.domain && c.name == cookie.name && c.path == cookie.path
        }) {
            *existing = cookie;
        } else {
            self.cookies.push(cookie);
        }
    }

    /// Cookie header value for a URL host + path.
    pub fn header_for(&self, host: &str, path: &str) -> Option<String> {
        let domain = host_to_domain(host);
        let matching: Vec<_> = self
            .cookies
            .iter()
            .filter(|c| domain_matches(&c.domain, &domain) && path.starts_with(&c.path))
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        if matching.is_empty() {
            None
        } else {
            Some(matching.join("; "))
        }
    }

    /// Remove cookie at index. Returns true if removed.
    pub fn remove_at(&mut self, index: usize) -> bool {
        if index < self.cookies.len() {
            self.cookies.remove(index);
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.cookies.clear();
    }
}

fn host_to_domain(host: &str) -> String {
    host.split(':').next().unwrap_or(host).to_lowercase()
}

fn domain_matches(cookie_domain: &str, host: &str) -> bool {
    host == cookie_domain || host.ends_with(&format!(".{cookie_domain}"))
}

fn parse_set_cookie(raw: &str) -> Option<(String, String, String)> {
    let first = raw.split(';').next()?.trim();
    let (name, value) = first.split_once('=')?;
    if name.is_empty() {
        return None;
    }
    let mut path = "/".to_string();
    for part in raw.split(';').skip(1) {
        let part = part.trim();
        if let Some(p) = part
            .strip_prefix("Path=")
            .or_else(|| part.strip_prefix("path="))
        {
            path = p.to_string();
        }
    }
    Some((name.to_string(), value.to_string(), path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_set_cookie() {
        let mut jar = CookieJar::default();
        jar.store_from_set_cookie(
            "httpbin.org",
            &["session=abc; Path=/".into()],
        );
        let hdr = jar.header_for("httpbin.org", "/get").unwrap();
        assert!(hdr.contains("session=abc"));
    }
}
