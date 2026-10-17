//! Local persistence — JSON snapshot under app data dir; optional git cloud sync.

#![allow(unused_imports)]

pub mod json_store;

pub use json_store::{AppSnapshot, JsonStore, StorageError};
