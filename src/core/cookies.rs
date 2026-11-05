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
        let fallback_domain = host_to_domain(host);
        for raw in set_cookie_values {
            if let Some(parsed) = parse_set_cookie(raw) {
                let domain = parsed.domain.unwrap_or_else(|| fallback_domain.clone());
                let domain = domain.strip_prefix('.').unwrap_or(&domain).to_lowercase();
                
                // If Max-Age is 0 or negative, remove expired cookie
                if parsed.max_age.map(|a| a <= 0).unwrap_or(false) {
                    self.delete_cookie(&domain, &parsed.name, &parsed.path);
                } else {
                    self.upsert(Cookie {
                        domain,
                        name: parsed.name,
                        value: parsed.value,
                        path: parsed.path,
                    });
                }
            }
        }
    }

    pub fn delete_cookie(&mut self, domain: &str, name: &str, path: &str) {
        let clean_d = domain.strip_prefix('.').unwrap_or(domain);
        self.cookies.retain(|c| {
            let c_domain = c.domain.strip_prefix('.').unwrap_or(&c.domain);
            !(c_domain.eq_ignore_ascii_case(clean_d) && c.name == name && c.path == path)
        });
    }

    pub fn upsert(&mut self, cookie: Cookie) {
        if let Some(existing) = self.cookies.iter_mut().find(|c| {
            c.domain.eq_ignore_ascii_case(&cookie.domain)
                && c.name == cookie.name
                && c.path == cookie.path
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
    let cd = cookie_domain.strip_prefix('.').unwrap_or(cookie_domain).to_lowercase();
    let h = host.strip_prefix('.').unwrap_or(host).to_lowercase();
    h == cd || h.ends_with(&format!(".{cd}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSetCookie {
    pub name: String,
    pub value: String,
    pub path: String,
    pub domain: Option<String>,
    pub max_age: Option<i64>,
}

fn parse_set_cookie(raw: &str) -> Option<ParsedSetCookie> {
    let first = raw.split(';').next()?.trim();
    let (name, value) = first.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let value = value.trim();
    let mut path = "/".to_string();
    let mut domain = None;
    let mut max_age = None;

    for part in raw.split(';').skip(1) {
        let part = part.trim();
        if let Some(p) = part
            .strip_prefix("Path=")
            .or_else(|| part.strip_prefix("path="))
        {
            path = p.trim().to_string();
        } else if let Some(d) = part
            .strip_prefix("Domain=")
            .or_else(|| part.strip_prefix("domain="))
        {
            domain = Some(d.trim().to_string());
        } else if let Some(ma) = part
            .strip_prefix("Max-Age=")
            .or_else(|| part.strip_prefix("max-age="))
        {
            max_age = ma.trim().parse::<i64>().ok();
        }
    }
    Some(ParsedSetCookie {
        name: name.to_string(),
        value: value.to_string(),
        path,
        domain,
        max_age,
    })
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

    #[test]
    fn test_cookie_domain_matching_and_max_age_expiration() {
        let mut jar = CookieJar::default();

        // 1. Domain attribute applies to subdomains
        jar.store_from_set_cookie(
            "auth.example.com",
            &["token=12345; Domain=.example.com; Path=/api".into()],
        );
        let hdr_sub = jar.header_for("api.example.com", "/api/v1").unwrap();
        assert_eq!(hdr_sub, "token=12345");

        // 2. Max-Age=0 deletes the cookie
        jar.store_from_set_cookie(
            "api.example.com",
            &["token=; Domain=.example.com; Path=/api; Max-Age=0".into()],
        );
        assert!(jar.header_for("api.example.com", "/api/v1").is_none());
    }
}
