//! Turning a command for a light or group into commands for the physical
//! lights behind it.

use anyhow::{bail, Result};

use crate::adapter::{Adapters, LightCommand};
use crate::model::{DeviceRef, Model};

/// Staging adds the next stage once the lights that are on would go above
/// this brightness…
const STAGE_UP: f64 = 95.0;
/// …and drops it again only once the smaller set would stay below this, so
/// lights don't flicker at the threshold.
const STAGE_DOWN: f64 = 75.0;

/// Which members of a staged group a command reaches: `on` share the group's
/// brightness, `off` are switched off, any others are left alone.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedLights {
    pub on: Vec<String>,
    pub off: Vec<String>,
}

/// How many leading `stages` carry the group `value` (0–100). `active` is
/// the count in use so far; without it the fewest stages that can carry the
/// value are chosen.
pub fn stage_count(stages: &[Vec<String>], value: f64, active: Option<usize>) -> usize {
    let total: usize = stages.iter().map(Vec::len).sum();
    let per_light = |count: usize| value * total as f64 / stages[..count].iter().map(Vec::len).sum::<usize>() as f64;
    let mut count = active.unwrap_or(1).clamp(1, stages.len());
    while count < stages.len() && per_light(count) > STAGE_UP {
        count += 1;
    }
    if active.is_some() {
        while count > 1 && per_light(count - 1) < STAGE_DOWN {
            count -= 1;
        }
    }
    count
}

/// The lights of the first `count` stages, and those to switch off: the
/// stages just dropped from `previous`, or all others without it.
pub fn staged_lights(stages: &[Vec<String>], count: usize, previous: Option<usize>) -> StagedLights {
    let lit_before = previous.map_or(stages.len(), |previous| previous.max(count));
    StagedLights { on: stages[..count].concat(), off: stages[count..lit_before].concat() }
}

/// Commands for every physical light behind `path`. A group's brightness is
/// spread over the lights that `staged` turns on (all members without it),
/// then passed through each member's brightness mapping; switching off stays off.
pub fn commands_for(
    model: &Model,
    path: &str,
    command: &LightCommand,
    staged: Option<&StagedLights>,
) -> Vec<(String, DeviceRef, LightCommand)> {
    if let Some(group) = model.groups.get(path) {
        let lit = staged.map_or(group.members.len(), |staged| staged.on.len());
        let spread = |brightness: f64| (brightness * group.members.len() as f64 / lit as f64).min(100.0);
        return group
            .members
            .iter()
            .filter_map(|member| {
                let light = model.lights.get(&member.light)?;
                let light_command = match staged {
                    Some(staged) if staged.off.contains(&member.light) => {
                        LightCommand { on: Some(false), transition: command.transition, ..Default::default() }
                    }
                    Some(staged) if !staged.on.contains(&member.light) => return None,
                    _ => LightCommand {
                        brightness: command.brightness.map(|b| member.brightness_for(spread(b))),
                        ..command.clone()
                    },
                };
                Some((light.path.clone(), light.device.clone(), light_command))
            })
            .collect();
    }
    match model.lights.get(path) {
        Some(light) => vec![(light.path.clone(), light.device.clone(), command.clone())],
        None => Vec::new(),
    }
}

/// Sends commands to all lights at once, so a group changes together.
/// Returns the paths of the lights whose command failed.
pub async fn send_all(adapters: &Adapters, commands: &[(String, DeviceRef, LightCommand)]) -> Vec<String> {
    let sends = commands.iter().map(|(path, device, command)| async move {
        match adapters.send(device, command).await {
            Ok(()) => None,
            Err(error) => {
                log::error!("{path}: {error:#}");
                Some(path.clone())
            }
        }
    });
    futures::future::join_all(sends).await.into_iter().flatten().collect()
}

/// Parses `knut set` values: `on`, `off`, a brightness like `40` or `40%`,
/// a colour temperature like `2700K`, and `transition=2` (seconds).
pub fn parse_values(values: &[String]) -> Result<LightCommand> {
    let mut command = LightCommand::default();
    if values.is_empty() {
        bail!("expected a value: on, off, a brightness like 40%, or a colour temperature like 2700K");
    }
    for value in values {
        let value = value.trim();
        let lower = value.to_ascii_lowercase();
        if lower == "on" {
            command.on = Some(true);
        } else if lower == "off" {
            command.on = Some(false);
        } else if let Some(seconds) = lower.strip_prefix("transition=") {
            command.transition = Some(parse_number(seconds, value)?);
        } else if let Some(kelvin) = lower.strip_suffix('k') {
            let kelvin = parse_number(kelvin, value)?;
            if !(1000.0..=10000.0).contains(&kelvin) {
                bail!("colour temperature {value} is outside 1000K–10000K");
            }
            command.kelvin = Some(kelvin);
        } else {
            let brightness = parse_number(lower.trim_end_matches('%'), value)?;
            if !(0.0..=100.0).contains(&brightness) {
                bail!("brightness {value} is outside 0–100");
            }
            if brightness == 0.0 {
                command.on = Some(false);
            } else {
                command.brightness = Some(brightness);
            }
        }
    }
    if command.on != Some(false) && (command.brightness.is_some() || command.kelvin.is_some()) {
        command.on = Some(true);
    }
    Ok(command)
}

fn parse_number(text: &str, original: &str) -> Result<f64> {
    match text.parse::<f64>() {
        Ok(number) if number.is_finite() => Ok(number),
        _ => bail!("cannot understand {original:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::Script;

    #[test]
    fn parses_set_values() {
        let values = |v: &[&str]| parse_values(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(values(&["off"]).unwrap(), LightCommand { on: Some(false), ..Default::default() });
        let command = values(&["40%", "2700K", "transition=2"]).unwrap();
        assert_eq!(command.on, Some(true));
        assert_eq!(command.brightness, Some(40.0));
        assert_eq!(command.kelvin, Some(2700.0));
        assert_eq!(command.transition, Some(2.0));
        assert!(values(&["bright"]).is_err());
        assert!(values(&["150"]).is_err());
    }

    #[test]
    fn group_commands_apply_brightness_mapping() {
        let config = r#"
zigbee "z2m", mqtt: "mqtt://localhost"
room "Küche" do
  light "spot1", zigbee: "0x01"
  light "spot3", zigbee: "0x03"
  group "Spots", lights: [:spot1, :spot3] do
    map :spot3, brightness: ->(x) { x * 0.8 }
  end
end
"#;
        let (_, model) = Script::load_str(config, "config.rb").unwrap();
        let command = LightCommand { on: Some(true), brightness: Some(50.0), ..Default::default() };
        let commands = commands_for(&model, "Küche/Spots", &command, None);
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].2.brightness, Some(50.0));
        assert_eq!(commands[1].2.brightness, Some(40.0));

        let off = LightCommand { on: Some(false), ..Default::default() };
        assert!(commands_for(&model, "Küche/Spots", &off, None).iter().all(|(_, _, c)| c.brightness.is_none()));
    }

    fn stages(sizes: &[usize]) -> Vec<Vec<String>> {
        let mut light = 0;
        sizes
            .iter()
            .map(|size| {
                (0..*size)
                    .map(|_| {
                        light += 1;
                        format!("R/l{light}")
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn picks_the_fewest_stages_that_carry_the_value() {
        let one_then_two = stages(&[1, 2]);
        // One of three lights carries up to 95 / 3 ≈ 31.7.
        for (value, expected) in [(5.0, 1), (31.0, 1), (32.0, 2), (100.0, 2)] {
            assert_eq!(stage_count(&one_then_two, value, None), expected, "value {value}");
        }
        let three = stages(&[1, 1, 1]);
        for (value, expected) in [(30.0, 1), (40.0, 2), (63.0, 2), (64.0, 3)] {
            assert_eq!(stage_count(&three, value, None), expected, "value {value}");
        }
    }

    #[test]
    fn staging_has_hysteresis() {
        let one_then_two = stages(&[1, 2]);
        // Up above 95 per light (value 31.7), down below 75 on the smaller set (value 25).
        let mut active = None;
        let mut sequence = Vec::new();
        for value in [20.0, 31.0, 32.0, 30.0, 26.0, 25.0, 24.9, 30.0, 31.0, 32.0] {
            let count = stage_count(&one_then_two, value, active);
            active = Some(count);
            sequence.push(count);
        }
        assert_eq!(sequence, [1, 1, 2, 2, 2, 2, 1, 1, 1, 2]);
    }

    #[test]
    fn staged_lights_only_switch_off_what_was_on() {
        let all = stages(&[1, 2, 1]);
        let names = |names: &[&str]| names.iter().map(|n| format!("R/{n}")).collect::<Vec<_>>();
        assert_eq!(staged_lights(&all, 1, None), StagedLights { on: names(&["l1"]), off: names(&["l2", "l3", "l4"]) });
        assert_eq!(staged_lights(&all, 1, Some(2)), StagedLights { on: names(&["l1"]), off: names(&["l2", "l3"]) });
        assert_eq!(staged_lights(&all, 2, Some(1)), StagedLights { on: names(&["l1", "l2", "l3"]), off: vec![] });
    }

    #[test]
    fn staging_spreads_the_value_before_the_mapping() {
        let config = r#"
zigbee "z2m", mqtt: "mqtt://localhost"
room "Küche" do
  light "spot1", zigbee: "0x01"
  light "spot2", zigbee: "0x02"
  light "spot3", zigbee: "0x03"
  group "Spots", lights: [:spot1, :spot2, :spot3] do
    stages [:spot2], [:spot1, :spot3]
    map :spot2, brightness: ->(x) { x / 2 }
  end
end
"#;
        let (_, model) = Script::load_str(config, "config.rb").unwrap();
        let stages = model.groups["Küche/Spots"].stages.clone().unwrap();
        let brightness_of = |value: f64, count: usize| {
            let command = LightCommand { on: Some(true), brightness: Some(value), ..Default::default() };
            let staged = staged_lights(&stages, count, None);
            commands_for(&model, "Küche/Spots", &command, Some(&staged))
                .into_iter()
                .map(|(path, _, c)| (path.rsplit('/').next().unwrap().to_string(), c.on, c.brightness))
                .collect::<Vec<_>>()
        };
        let on = |name: &str, brightness| (name.to_string(), Some(true), Some(brightness));
        let off = |name: &str| (name.to_string(), Some(false), None);
        // value · n / k = 20 · 3 / 1 = 60, then spot2's mapping halves it.
        assert_eq!(brightness_of(20.0, 1), [off("spot1"), on("spot2", 30.0), off("spot3")]);
        assert_eq!(brightness_of(50.0, 1), [off("spot1"), on("spot2", 50.0), off("spot3")], "capped at 100 before mapping");
        assert_eq!(brightness_of(60.0, 2), [on("spot1", 60.0), on("spot2", 30.0), on("spot3", 60.0)]);
    }
}
