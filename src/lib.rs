//! Knut: a small smart-home daemon for lights, rooms, groups, scenes and
//! scripted automations, exposed to Apple Home.
//!
//! The library holds everything behind the `knut` binary, which only parses
//! the command line. Its public API is the five commands re-exported below;
//! every module is private to the crate. See CONTRIBUTE.md for how the
//! modules fit together.

#![warn(unreachable_pub)]

mod adapter;
mod commands;
mod control;
mod engine;
mod homekit;
mod model;
mod script;
mod store;
mod sun;

pub use commands::{check, discover, run, set, unpair};
