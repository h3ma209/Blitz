//! Secrets backends — OS keychain + in-memory fallback.
//!
//! # Naming (Postman CryptoService / keytar pattern)
//!
//! | Field   | Value                                      |
//! |---------|--------------------------------------------|
//! | service | `Blitz ({account_id})`                |
//! | account | `userKey` by default ([`DEFAULT_ACCOUNT`]) |
//!
//! Auth / header fields may use vault-like refs: `secret://{account_id}` or
//! `secret://{account_id}/{account}` — resolved in the send pipeline when present.
//!
//! Force in-memory (tests/CI): `BLITZ_SECRETS=memory`.

#![allow(unused_imports)]

pub mod keychain;

pub use keychain::{
    is_secret_ref, open_default_store, parse_secret_ref, resolve_secret_value,
    service_name, InMemorySecrets, SecretError, SecretRef, SecretsStore, APP_NAME,
    DEFAULT_ACCOUNT, SECRETS_BACKEND_ENV,
};
#[cfg(test)]
pub use keychain::{retrieve_user_key, store_user_key};

#[cfg(target_os = "macos")]
pub use keychain::OsKeychainSecrets;
