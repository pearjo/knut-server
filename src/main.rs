//! Knut: a small smart-home daemon for lights, rooms, groups, scenes and
//! scripted automations, exposed to Apple Home.

mod adapter;
mod control;
mod engine;
mod homekit;
mod model;
mod script;
mod store;
mod sun;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tokio::sync::mpsc;

use adapter::{Adapter, Adapters};
use model::{AdapterConfig, Model};
use script::Script;

#[derive(Parser)]
#[command(version, about = "Knut, a small smart-home daemon for Apple Home")]
struct Cli {
    /// The Ruby config.
    #[arg(short, long, global = true, env = "KNUT_CONFIG", default_value = "/etc/knut/config.rb")]
    config: PathBuf,

    /// Where pairing keys and auto/override state are kept.
    #[arg(long, global = true, env = "KNUT_STATE_DIR", default_value = "/var/lib/knut")]
    state_dir: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse and validate a config, then print the model.
    Check {
        /// Defaults to --config.
        config: Option<PathBuf>,
    },
    /// List devices not referenced in the config, as ready-to-paste DSL.
    Discover,
    /// Control a light or group, e.g. `knut set Küche/Spots 40% 2700K`.
    Set {
        /// "Room/Entity".
        entity: String,
        /// on, off, a brightness (40 or 40%), a colour temperature (2700K),
        /// transition=<seconds>.
        #[arg(required = true, num_args = 1..)]
        values: Vec<String>,
    },
    /// Run the daemon.
    Run,
    /// Forget all HomeKit pairings, so the bridge can be added to Apple Home
    /// again. Run it while the daemon is stopped.
    Unpair,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,hap=warn,libmdns=error")).init();

    let runtime = tokio::runtime::Runtime::new().expect("cannot start the async runtime");
    let result = match cli.command {
        Command::Check { config } => check(config.as_deref().unwrap_or(&cli.config)),
        Command::Discover => runtime.block_on(discover(&cli.config, &cli.state_dir)),
        Command::Set { entity, values } => runtime.block_on(set(&cli.config, &cli.state_dir, &entity, &values)),
        Command::Run => runtime.block_on(run(&cli.config, &cli.state_dir)),
        Command::Unpair => homekit::unpair(&cli.state_dir),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

fn load(config: &Path) -> Result<(Script, Model)> {
    Script::load(config).map_err(|error| anyhow::anyhow!("{error}"))
}

fn check(config: &Path) -> Result<()> {
    let (_, model) = load(config)?;
    print!("{model}");
    Ok(())
}

async fn discover(config: &Path, state_dir: &Path) -> Result<()> {
    let (_, model) = load(config)?;
    for adapter in &model.adapters {
        if let AdapterConfig::Hue { name, host, key: None, .. } = adapter {
            if adapter::hue::load_key(state_dir, name)?.is_none() {
                pair_hue(state_dir, name, host).await?;
            }
        }
    }

    let (events, _) = mpsc::unbounded_channel();
    let adapters = Adapters::connect(&model, state_dir, events).await?;
    let referenced: HashSet<&model::DeviceRef> =
        model.lights.values().map(|l| &l.device).chain(model.sensors.values().map(|s| &s.device)).collect();

    let mut failed = false;
    for config in &model.adapters {
        let adapter = adapters.get(config.name()).expect("every configured adapter is connected");
        println!("# {} {:?}", config.kind(), config.name());
        match adapter.discover().await {
            Ok(devices) => {
                let new: Vec<_> = devices.into_iter().filter(|d| !referenced.contains(&d.device)).collect();
                if new.is_empty() {
                    println!("# every device is already in the config");
                }
                for device in new {
                    println!("{:<60} # {}", adapter::dsl_snippet(&device, &model), device.description.trim());
                }
            }
            Err(error) => {
                println!("# {error:#}");
                failed = true;
            }
        }
        println!();
    }
    if failed {
        bail!("some adapters could not be queried");
    }
    Ok(())
}

async fn pair_hue(state_dir: &Path, name: &str, host: &str) -> Result<()> {
    eprintln!("hue {name:?} is not paired: press the link button on the bridge at {host} (waiting 30 s)");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        match adapter::hue::pair(state_dir, name, host).await {
            Ok(_) => {
                eprintln!("hue {name:?}: paired");
                return Ok(());
            }
            Err(error) if tokio::time::Instant::now() >= deadline => return Err(error),
            Err(_) => tokio::time::sleep(Duration::from_secs(2)).await,
        }
    }
}

async fn set(config: &Path, state_dir: &Path, entity: &str, values: &[String]) -> Result<()> {
    let (_, model) = load(config)?;
    if model.entity_kind(entity).is_none() {
        bail!("unknown light or group {entity:?}");
    }
    let command = control::parse_values(values)?;
    let stages = model.groups.get(entity).and_then(|group| group.stages.as_ref());
    let staged = match (stages, command.brightness) {
        (Some(stages), Some(brightness)) => {
            Some(control::staged_lights(stages, control::stage_count(stages, brightness, None), None))
        }
        _ => None,
    };
    let commands = control::commands_for(&model, entity, &command, staged.as_ref());

    let (events, _) = mpsc::unbounded_channel();
    let adapters = Adapters::connect(&model, state_dir, events).await?;
    let failed = control::send_all(&adapters, &commands).await;
    // MQTT publishes leave from the client's background task; give it time.
    if commands.iter().any(|(_, device, _)| matches!(adapters.get(&device.adapter), Some(Adapter::Zigbee(_)))) {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    if !failed.is_empty() {
        bail!("could not reach {}", failed.join(", "));
    }
    Ok(())
}

async fn run(config: &Path, state_dir: &Path) -> Result<()> {
    let (script, model) = load(config)?;
    let model = Arc::new(model);
    log::info!(
        "loaded {}: {} lights, {} groups, {} scenes, {} triggers",
        config.display(),
        model.lights.len(),
        model.groups.len(),
        model.scenes.len(),
        model.triggers.len()
    );
    let mut store = store::Store::open(state_dir)?;

    let (adapter_events_tx, adapter_events) = mpsc::unbounded_channel();
    let adapters = Adapters::connect(&model, state_dir, adapter_events_tx).await?;

    let (inputs_tx, inputs) = mpsc::unbounded_channel();
    let (updates_tx, updates) = mpsc::unbounded_channel();
    homekit::start(&model, &mut store, state_dir, inputs_tx, updates).await.context("cannot start HomeKit")?;

    let engine = engine::Engine::new(Arc::clone(&model), script, adapters, store, updates_tx);
    // mruby is single-threaded, so the engine stays on this thread.
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            tokio::select! {
                _ = engine.run(inputs, adapter_events) => {}
                _ = tokio::signal::ctrl_c() => log::info!("shutting down"),
            }
        })
        .await;
    Ok(())
}
