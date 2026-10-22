//! Turning a command for a light or group into commands for the physical
//! lights behind it.

use anyhow::{bail, Result};

use crate::adapter::{Adapters, LightCommand};
use crate::model::{DeviceRef, Model};

/// Commands for every physical light behind `path`. A group's brightness is
/// passed through each member's brightness mapping; switching off stays off.
pub fn commands_for(model: &Model, path: &str, command: &LightCommand) -> Vec<(String, DeviceRef, LightCommand)> {
    if let Some(group) = model.groups.get(path) {
        return group
            .members
            .iter()
            .filter_map(|member| {
                let light = model.lights.get(&member.light)?;
                let mut light_command = command.clone();
                light_command.brightness = command.brightness.map(|b| member.brightness_for(b));
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
        let commands = commands_for(&model, "Küche/Spots", &command);
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].2.brightness, Some(50.0));
        assert_eq!(commands[1].2.brightness, Some(40.0));

        let off = LightCommand { on: Some(false), ..Default::default() };
        assert!(commands_for(&model, "Küche/Spots", &off).iter().all(|(_, _, c)| c.brightness.is_none()));
    }
}
