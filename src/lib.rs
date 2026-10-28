//! <img src="https://raw.githubusercontent.com/pearjo/knut-server/devel/knut.png"
//!      alt="Knut" width="120" align="right">
//!
//! Knut: a small smart-home daemon for lights, rooms, groups, scenes and
//! scripted automations, exposed to Apple Home.
//!
//! The whole home is described in a Ruby config (see `docs/dsl.md`). Knut
//! loads it into a [`Model`](model::Model), drives the lights through
//! zigbee2mqtt and Philips Hue, and shows groups and scenes in the Home app.
//! `DESIGN.md` explains what Knut does and why; `CONTRIBUTE.md` explains how
//! the code is organised and how it should read.
//!
//! # Where to start
//!
//! The `knut` binary (`src/main.rs`) only parses the command line and calls
//! one of the five commands in [`commands`], re-exported here:
//!
//! | Command      | What it does                                                      |
//! |--------------|-------------------------------------------------------------------|
//! | [`check`]    | Loads and validates a config, then prints the model.              |
//! | [`discover`] | Prints the devices the config does not use yet, as DSL lines.     |
//! | [`set`]      | Sets a light or group once, without the daemon.                   |
//! | [`run`]      | Runs the daemon: adapters, HomeKit bridge and engine.             |
//! | [`unpair`]   | Forgets every HomeKit pairing.                                    |
//!
//! To follow the daemon, read [`run`] first. It loads the config with
//! [`Script::load`](script::Script::load), connects the
//! [`Adapters`](adapter::Adapters), starts the bridge with
//! [`homekit::start`] and hands everything to
//! [`Engine::run`](engine::Engine::run), which reacts to HomeKit, devices,
//! the sun and the clock from then on.
//!
//! # How the modules fit together
//!
//! ```text
//! config.rb ──> script ──> model ──> engine ──> control ──> adapter ──> lights
//!                                     ^  │
//!                  homekit ───────────┘  └──> store
//! ```
//!
//! | Module      | Responsible for                                                         |
//! |-------------|-------------------------------------------------------------------------|
//! | [`script`]  | Running the config in mruby and the scene and trigger blocks.           |
//! | [`model`]   | The configured home as plain data, validated as a whole.                |
//! | [`engine`]  | Entity state, auto mode, scenes and triggers.                           |
//! | [`control`] | Turning a group or light state into per-device commands.                |
//! | [`adapter`] | Talking to devices: [`zigbee`](adapter::zigbee) and [`hue`](adapter::hue). |
//! | [`homekit`] | The HAP bridge; it talks only to the engine, never to adapters.         |
//! | [`sun`]     | Sun position, computed locally.                                         |
//! | [`store`]   | Persistent state: accessory ids and auto flags.                         |
//!
//! Every module is private to the crate. Open these docs with
//! `cargo doc --document-private-items --open` to see them.

#![doc(html_logo_url = "https://raw.githubusercontent.com/pearjo/knut-server/devel/knut.png")]
#![warn(unreachable_pub)]
// The guide above links into the private modules; it is read with
// `--document-private-items`, where those links resolve.
#![allow(rustdoc::private_intra_doc_links)]

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
