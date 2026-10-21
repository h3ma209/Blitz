//! OS keychain-backed secrets (Postman CryptoService / keytar pattern).
//!
//! # Service / account naming
//!
//! Mirrors Postman's observed CryptoService layout (reports/05, reports/08):
//! - **service**: `{APP_NAME} ({account_id})` — e.g. `Blitz (local)`
//! - **account**: typically [`DEFAULT_ACCOUNT`] (`userKey`) for the local user key;
//!   other account labels may store bearer tokens, etc.
//!
//! # Backends
//!
//! - [`OsKeychainSecrets`] — macOS Keychain via the `keyring` crate (`apple-native`).
//! - [`InMemorySecrets`] — process-local map; used in tests/CI or when keychain
//!   is unavailable / forced via `BLITZ_SECRETS=memory`.
//!
//! Prefer [`open_default_store`] at call sites.

use std::collections::HashMap;
use std::sync::Mutex;

use thiserror::Error;

/// App name embedded in Keychain service strings (Postman uses `app.getName()`).
pub const APP_NAME: &str = "Blitz";

/// Default Keychain account label for the local encryption / user key
/// (Postman CryptoService uses the literal `"userKey"`).
pub const DEFAULT_ACCOUNT: &str = "userKey";

/// Env var: set to `memory` to force the in-memory backend (CI / headless).
pub const SECRETS_BACKEND_ENV: &str = "BLITZ_SECRETS";

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("not found: {0}")]
    NotFound(String),
    #[allow(dead_code)]
    #[error("backend unavailable: {0}")]
    Unavailable(String),
    #[error("invalid secret reference: {0}")]
    InvalidRef(String),
}

/// Abstraction over secret backends (keychain / vault / 1Password later).
#[allow(dead_code)]
pub trait SecretsStore: Send + Sync {
    fn get(&self, service: &str, account: &str) -> Result<String, SecretError>;
    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), SecretError>;
    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError>;
}

/// Build Keychain service name: `Blitz ({account_id})`.
pub fn service_name(account_id: &str) -> String {
    format!("{APP_NAME} ({account_id})")
}

/// Store under [`DEFAULT_ACCOUNT`] for `account_id`.
#[cfg(test)]
pub fn store_user_key(
    store: &dyn SecretsStore,
    account_id: &str,
    secret: &str,
) -> Result<(), SecretError> {
    store.set(&service_name(account_id), DEFAULT_ACCOUNT, secret)
}

/// Retrieve [`DEFAULT_ACCOUNT`] for `account_id`.
#[cfg(test)]
pub fn retrieve_user_key(
    store: &dyn SecretsStore,
    account_id: &str,
) -> Result<String, SecretError> {
    store.get(&service_name(account_id), DEFAULT_ACCOUNT)
}

/// In-memory store — never persists; tests, CI, and keychain fallback.
#[derive(Debug, Default)]
pub struct InMemorySecrets {
    inner: Mutex<HashMap<(String, String), String>>,
}

impl SecretsStore for InMemorySecrets {
    fn get(&self, service: &str, account: &str) -> Result<String, SecretError> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(service.to_string(), account.to_string()))
            .cloned()
            .ok_or_else(|| SecretError::NotFound(format!("{service}/{account}")))
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), SecretError> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((service.to_string(), account.to_string()), secret.to_string());
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&(service.to_string(), account.to_string()));
        Ok(())
    }
}

/// macOS Keychain backend (generic password via `keyring` → Security.framework).
#[cfg(target_os = "macos")]
#[derive(Debug, Default, Clone, Copy)]
pub struct OsKeychainSecrets;

#[cfg(target_os = "macos")]
impl OsKeychainSecrets {
    /// Probe whether Keychain entry creation works on this machine.
    pub fn available() -> bool {
        keyring::Entry::new(&service_name("__probe__"), DEFAULT_ACCOUNT).is_ok()
    }
}

#[cfg(target_os = "macos")]
impl SecretsStore for OsKeychainSecrets {
    fn get(&self, service: &str, account: &str) -> Result<String, SecretError> {
        let entry = keyring::Entry::new(service, account).map_err(map_keyring_err)?;
        match entry.get_password() {
            Ok(s) => Ok(s),
            Err(keyring::Error::NoEntry) => {
                Err(SecretError::NotFound(format!("{service}/{account}")))
            }
            Err(e) => Err(map_keyring_err(e)),
        }
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), SecretError> {
        let entry = keyring::Entry::new(service, account).map_err(map_keyring_err)?;
        entry.set_password(secret).map_err(map_keyring_err)
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError> {
        let entry = keyring::Entry::new(service, account).map_err(map_keyring_err)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring_err(e)),
        }
    }
}

#[cfg(target_os = "macos")]
fn map_keyring_err(err: keyring::Error) -> SecretError {
    SecretError::Unavailable(err.to_string())
}

/// Open preferred secrets backend: OS keychain when usable, else in-memory.
///
/// Force memory with `EVILPOSTMAN_SECRETS=memory`.
pub fn open_default_store() -> Box<dyn SecretsStore> {
    if force_memory_backend() {
        return Box::new(InMemorySecrets::default());
    }

    #[cfg(target_os = "macos")]
    {
        if OsKeychainSecrets::available() {
            return Box::new(OsKeychainSecrets);
        }
    }

    Box::new(InMemorySecrets::default())
}

fn force_memory_backend() -> bool {
    std::env::var(SECRETS_BACKEND_ENV)
        .map(|v| v.eq_ignore_ascii_case("memory"))
        .unwrap_or(false)
}

/// Parsed `secret://…` reference used in auth / header fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRef {
    pub account_id: String,
    pub account: String,
}

impl SecretRef {
    /// Keychain service string for this ref.
    pub fn service(&self) -> String {
        service_name(&self.account_id)
    }
}

/// True when `value` looks like a vault/keychain ref (`secret://…`).
pub fn is_secret_ref(value: &str) -> bool {
    value.trim().starts_with("secret://")
}

/// Parse `secret://{account_id}` or `secret://{account_id}/{account}`.
///
/// Bare `secret://{account_id}` uses [`DEFAULT_ACCOUNT`] (`userKey`).
pub fn parse_secret_ref(value: &str) -> Result<SecretRef, SecretError> {
    let trimmed = value.trim();
    let rest = trimmed
        .strip_prefix("secret://")
        .ok_or_else(|| SecretError::InvalidRef(value.to_string()))?;

    if rest.is_empty() {
        return Err(SecretError::InvalidRef(
            "empty secret:// reference".into(),
        ));
    }

    let (account_id, account) = match rest.split_once('/') {
        Some((id, acct)) if !id.is_empty() && !acct.is_empty() => (id, acct),
        None if !rest.contains('/') => (rest, DEFAULT_ACCOUNT),
        _ => {
            return Err(SecretError::InvalidRef(format!(
                "expected secret://{{id}} or secret://{{id}}/{{account}}, got {value:?}"
            )));
        }
    };

    Ok(SecretRef {
        account_id: account_id.to_string(),
        account: account.to_string(),
    })
}

/// If `value` is a `secret://` ref, resolve it; otherwise return `value` unchanged.
pub fn resolve_secret_value(
    store: &dyn SecretsStore,
    value: &str,
) -> Result<String, SecretError> {
    if !is_secret_ref(value) {
        return Ok(value.to_string());
    }
    let r = parse_secret_ref(value)?;
    store.get(&r.service(), &r.account)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_name_matches_postman_pattern() {
        assert_eq!(service_name("abc-123"), "Blitz (abc-123)");
    }

    #[test]
    fn memory_roundtrip() {
        let store = InMemorySecrets::default();
        store
            .set(&service_name("local"), DEFAULT_ACCOUNT, "s3cret")
            .unwrap();
        assert_eq!(
            store
                .get(&service_name("local"), DEFAULT_ACCOUNT)
                .unwrap(),
            "s3cret"
        );
        store
            .delete(&service_name("local"), DEFAULT_ACCOUNT)
            .unwrap();
        assert!(matches!(
            store.get(&service_name("local"), DEFAULT_ACCOUNT),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn user_key_helpers() {
        let store = InMemorySecrets::default();
        store_user_key(&store, "team-1", "key-bytes").unwrap();
        assert_eq!(retrieve_user_key(&store, "team-1").unwrap(), "key-bytes");
    }

    #[test]
    fn parse_secret_refs() {
        let r = parse_secret_ref("secret://local").unwrap();
        assert_eq!(r.account_id, "local");
        assert_eq!(r.account, DEFAULT_ACCOUNT);

        let r = parse_secret_ref("secret://local/bearer").unwrap();
        assert_eq!(r.account_id, "local");
        assert_eq!(r.account, "bearer");

        assert!(parse_secret_ref("plain-token").is_err());
        assert!(parse_secret_ref("secret://").is_err());
        assert!(parse_secret_ref("secret://id/").is_err());
    }

    #[test]
    fn resolve_secret_value_memory() {
        let store = InMemorySecrets::default();
        store
            .set(&service_name("local"), "bearer", "tok-xyz")
            .unwrap();
        assert_eq!(
            resolve_secret_value(&store, "secret://local/bearer").unwrap(),
            "tok-xyz"
        );
        assert_eq!(
            resolve_secret_value(&store, "literal").unwrap(),
            "literal"
        );
    }

    #[test]
    fn open_default_store_respects_memory_env() {
        // SAFETY: test-only; serial within this module's tests for this env key.
        std::env::set_var(SECRETS_BACKEND_ENV, "memory");
        let store = open_default_store();
        store.set("svc", "acct", "v").unwrap();
        assert_eq!(store.get("svc", "acct").unwrap(), "v");
        std::env::remove_var(SECRETS_BACKEND_ENV);
    }

    /// Writes a real Keychain item — run manually: `cargo test keychain_roundtrip -- --ignored`
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "touches real macOS Keychain; interactive / machine-local"]
    fn keychain_roundtrip() {
        let store = OsKeychainSecrets;
        let svc = service_name("__blitz_test__");
        let acct = "unit-test";
        let _ = store.delete(&svc, acct);
        store.set(&svc, acct, "roundtrip-value").unwrap();
        assert_eq!(store.get(&svc, acct).unwrap(), "roundtrip-value");
        store.delete(&svc, acct).unwrap();
        assert!(matches!(
            store.get(&svc, acct),
            Err(SecretError::NotFound(_))
        ));
    }
}
