//! Local persistence stub — JSON files under app data dir (no cloud sync).

#![allow(unused_imports)]

pub mod json_store;

pub use json_store::{AppSnapshot, JsonStore, StorageError};
