//! RideShell core library.
//!
//! This crate contains everything that is independent of the command-line
//! front end: storage, profile resolution, environment planning, identity
//! adapters ("providers") and shell adapters. The CLI crate is a thin layer
//! on top of it, so other front ends (a TUI, an extension host) can reuse it.

pub mod brand;
pub mod config;
pub mod dialect;
pub mod environment;
pub mod envstate;
pub mod exec;
pub mod history;
pub mod names;
pub mod pathkey;
pub mod paths;
pub mod platform;
pub mod profile_file;
pub mod providers;
pub mod recent;
pub mod resolve;
pub mod secrets;
pub mod shells;
pub mod shims;
pub mod store;
pub mod tools;
pub mod vault;
