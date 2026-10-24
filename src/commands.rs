//! The commands of the `knut` binary. Each one loads the config and wires
//! the modules together for one job; the work itself happens in the modules.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::sync::mpsc;

use crate::adapter::{self, Adapter, Adapters};
use crate::model::{self, AdapterConfig, Model, Staging};
use crate::script::{self, Script};
use crate::{control, engine, homekit, store};

/// Loads and validates `config`, then prints the model.
pub fn check(config: &Path) -> Result<()> {
    let (_, model) = load(config)?;
    print!("{model}");
    Ok(())
}

/// Prints every device the adapters know but the config does not reference,
/// as ready-to-paste DSL. Pairs unpaired Hue bridges first.
pub async fn discover(config: &Path, state_dir: &Path) -> Result<()> {
    let (_, model) = load(config)?;
    for adapter in &model.adapters {
        if let AdapterConfig::Hue {
            name,
            host,
            key: None,
            ..
        } = adapter
        {
            if adapter::hue::load_key(state_dir, name)?.is_none() {
                pair_hue(state_dir, name, host).await?;
            }
        }
    }

    let (events, _) = mpsc::unbounded_channel();
    let adapters = Adapters::connect(&model, state_dir, events).await?;
    let referenced: HashSet<&model::DeviceRef> = model
        .lights
        .values()
        .map(|l| &l.device)
        .chain(model.sensors.values().map(|s| &s.device))
        .collect();

    let mut failed = false;
    for config in &model.adapters {
        let adapter = adapters
            .get(config.name())
            .expect("every configured adapter is connected");
        println!("# {} {:?}", config.kind(), config.name());
        match adapter.discover().await {
            Ok(devices) => {
                let new: Vec<_> = devices
                    .into_iter()
                    .filter(|d| !referenced.contains(&d.device))
                    .collect();
                if new.is_empty() {
                    println!("# every device is already in the config");
                }
                for device in new {
                    println!(
                        "{:<60} # {}",
                        adapter::dsl_snippet(&device, &model),
                        device.description.trim()
                    );
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

/// Sets the light or group at `entity` (a "Room/Entity" path) once, from
/// command-line `values` such as `on`, `40%`, `2700K` or `transition=2`.
/// Groups are staged and mapped as in the daemon, without hysteresis.
pub async fn set(config: &Path, state_dir: &Path, entity: &str, values: &[String]) -> Result<()> {
    let (mut script, model) = load(config)?;
    if model.entity_kind(entity).is_none() {
        bail!("unknown light or group {entity:?}");
    }
    let command = control::parse_values(values)?;
    let group = model.groups.get(entity);
    let staged = match (
        group.and_then(|group| group.staging.as_ref()),
        command.brightness,
    ) {
        (Some(Staging::Stages(stages)), Some(brightness)) => Some(control::staged_lights(
            stages,
            control::stage_count(stages, brightness, None),
            None,
        )),
        (Some(Staging::Custom { proc_id, line }), Some(brightness)) => {
            let group = group.expect("only groups have staging");
            let names = script
                .staging(
                    *proc_id,
                    brightness / 100.0,
                    chrono::Local::now().timestamp(),
                )
                .map_err(|error| {
                    anyhow::anyhow!("staging block at {line}: {}", script::describe(&error))
                })?;
            let on = control::picked_lights(group, &names)
                .map_err(|error| anyhow::anyhow!("{line}: {error}"))?;
            let off = group
                .members
                .iter()
                .map(|m| m.light.clone())
                .filter(|light| !on.contains(light))
                .collect();
            Some(control::StagedLights { on, off })
        }
        _ => None,
    };
    let commands = control::commands_for(&model, entity, &command, staged.as_ref());

    let (events, _) = mpsc::unbounded_channel();
    let adapters = Adapters::connect(&model, state_dir, events).await?;
    let failed = control::send_all(&adapters, &commands).await;
    // MQTT publishes leave from the client's background task; give it time.
    if commands
        .iter()
        .any(|(_, device, _)| matches!(adapters.get(&device.adapter), Some(Adapter::Zigbee(_))))
    {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    if !failed.is_empty() {
        bail!("could not reach {}", failed.join(", "));
    }
    Ok(())
}

/// Runs the daemon until Ctrl-C: connects the adapters, starts the HomeKit
/// bridge and hands control to the engine.
pub async fn run(config: &Path, state_dir: &Path) -> Result<()> {
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
    homekit::start(&model, &mut store, state_dir, inputs_tx, updates)
        .await
        .context("cannot start HomeKit")?;

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

/// Forgets every HomeKit pairing in `state_dir`, so the bridge can be added
/// to Apple Home again. The daemon must not be running.
pub fn unpair(state_dir: &Path) -> Result<()> {
    homekit::unpair(state_dir)
}

fn load(config: &Path) -> Result<(Script, Model)> {
    Script::load(config).map_err(|error| anyhow::anyhow!("{error}"))
}

async fn pair_hue(state_dir: &Path, name: &str, host: &str) -> Result<()> {
    eprintln!(
        "hue {name:?} is not paired: press the link button on the bridge at {host} (waiting 30 s)"
    );
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
