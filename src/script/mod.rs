//! Loads the Ruby DSL and runs scene and trigger blocks.

pub mod mruby;

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::model::{Model, RawModel, ValidationErrors};
use mruby::{Mruby, RubyError};

const PRELUDE: &str = include_str!("prelude.rb");
const PRELUDE_FILE: &str = "(knut)";

/// Something a scene or trigger block asked for.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Action {
    Set {
        targets: Vec<String>,
        on: Option<bool>,
        brightness: Option<f64>,
        kelvin: Option<f64>,
        transition: Option<f64>,
    },
    Adapt {
        targets: Vec<String>,
        brightness: Option<f64>,
        kelvin: Option<f64>,
        transition: Option<f64>,
    },
    Activate {
        scene: String,
    },
    Log {
        message: String,
    },
}

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Ruby(RubyError),
    Invalid(ValidationErrors),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot read config: {error}"),
            Self::Ruby(error) => write!(f, "{}", describe(error)),
            Self::Invalid(errors) => write!(f, "{errors}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Formats a Ruby error for people: config errors raised by the prelude
/// already name their config line, so the prelude's own location and
/// backtrace are noise.
pub fn describe(error: &RubyError) -> String {
    let message = error.message.as_str();
    let config_error = message.ends_with("(Knut::ConfigError)");
    let message = match message.strip_prefix(PRELUDE_FILE) {
        Some(rest) => rest.split_once(": ").map_or(rest, |(_, rest)| rest),
        None => message,
    };
    let message = message.trim_end_matches(" (Knut::ConfigError)");
    if config_error {
        return message.to_string();
    }
    let user_frames: Vec<&String> = error.backtrace.iter().filter(|line| !line.starts_with(PRELUDE_FILE)).collect();
    let mut text = message.to_string();
    for frame in user_frames {
        text.push_str("\n    from ");
        text.push_str(frame);
    }
    text
}

/// The config's Ruby interpreter, kept alive to run scene and trigger blocks.
pub struct Script {
    ruby: Mruby,
}

impl Script {
    /// Loads and validates the config at `path`.
    pub fn load(path: &Path) -> Result<(Self, Model), LoadError> {
        let code = std::fs::read_to_string(path).map_err(LoadError::Io)?;
        let filename = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into());
        Self::load_str(&code, &filename)
    }

    pub fn load_str(code: &str, filename: &str) -> Result<(Self, Model), LoadError> {
        let mut ruby = Mruby::new();
        ruby.eval(PRELUDE, PRELUDE_FILE).map_err(LoadError::Ruby)?;
        ruby.eval(code, filename).map_err(LoadError::Ruby)?;

        let json = ruby.eval("Knut.model_json", PRELUDE_FILE).map_err(LoadError::Ruby)?;
        // The prelude checks shapes; what is left are value errors such as a bad IP address.
        let raw: RawModel = serde_json::from_str(&json)
            .map_err(|error| LoadError::Invalid(ValidationErrors(vec![format!("{filename}: {error}")])))?;

        let mut tables = HashMap::new();
        for id in raw.mapping_procs() {
            let json = ruby.eval(&format!("Knut.mapping_table({id})"), PRELUDE_FILE).map_err(LoadError::Ruby)?;
            let table: Vec<f64> = serde_json::from_str(&json).expect("the prelude emits a JSON array");
            tables.insert(id, table);
        }
        let model = raw.build(&tables).map_err(LoadError::Invalid)?;

        let mut script = Self { ruby };
        let errors = script.dry_run_scenes(&model);
        if !errors.is_empty() {
            return Err(LoadError::Invalid(ValidationErrors(errors)));
        }
        Ok((script, model))
    }

    /// Runs proc `proc_id` with `args`, a comma separated list of Ruby
    /// expressions, and returns the actions it requested. `state` is the
    /// entity snapshot exposed through `state(path)` and `on?(path)`.
    pub fn run(&mut self, proc_id: usize, args: &str, state: &serde_json::Value) -> Result<Vec<Action>, RubyError> {
        let code = format!("Knut.run({proc_id}, [{args}], {})", mruby::ruby_literal(state));
        let json = self.ruby.eval(&code, PRELUDE_FILE)?;
        Ok(serde_json::from_str(&json).expect("the prelude emits valid action JSON"))
    }

    /// Scenes are almost always static, so running them once at load time
    /// catches typos in their targets before the daemon starts.
    fn dry_run_scenes(&mut self, model: &Model) -> Vec<String> {
        let empty = serde_json::json!({});
        let mut errors = Vec::new();
        for scene in &model.scenes {
            match self.run(scene.proc_id, "", &empty) {
                Ok(actions) => {
                    for problem in validate_actions(model, &actions) {
                        errors.push(format!("{}: scene {:?}: {problem}", scene.line, scene.name));
                    }
                }
                Err(error) => errors.push(format!("{}: scene {:?}: {}", scene.line, scene.name, describe(&error))),
            }
        }
        errors
    }
}

/// Unknown targets and scenes in a list of actions.
pub fn validate_actions(model: &Model, actions: &[Action]) -> Vec<String> {
    let mut problems = Vec::new();
    for action in actions {
        match action {
            Action::Set { targets, .. } | Action::Adapt { targets, .. } => {
                for target in targets {
                    if model.entity_kind(target).is_none() {
                        problems.push(format!("unknown light or group {target:?}"));
                    }
                }
            }
            Action::Activate { scene } => {
                if model.scene(scene).is_none() {
                    problems.push(format!("unknown scene {scene:?}"));
                }
            }
            Action::Log { .. } => {}
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
location lat: 52.99, lon: 10.58

hue    "bridge", host: "192.168.1.20"
zigbee "z2m",    mqtt: "mqtt://localhost:1883"

room "Küche" do
  light "spot1", zigbee: "0x01"
  light "spot2", zigbee: "0x02"
  light "spot3", zigbee: "0x03"
  light "decke", hue: "4f3c"

  group "Spots", lights: [:spot1, :spot2, :spot3] do
    map :spot3, brightness: ->(x) { x * 0.8 }
  end
end

room "Flur" do
  light "Licht", zigbee: "0x10"
  sensor "Bewegung", zigbee: "0x11"
end

scene "Filmabend", auto: false do
  set "Küche/Spots", brightness: 20, kelvin: 2200
end

scene "Morgen", auto: true do
  turn_on "Küche/Spots", "Flur/Licht"
end

on "Clock", at: "06:30" do
  activate "Morgen"
end

on "Flur/Bewegung", :motion do |event, now|
  turn_on "Flur/Licht" unless on?("Flur/Licht")
end

on "Sun", :sun do |sun, now|
  adapt all_groups, kelvin: sun.elevation > 10 ? 4000 : 2400, transition: 60
end
"#;

    #[test]
    fn loads_the_design_example() {
        let (mut script, model) = Script::load_str(CONFIG, "config.rb").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(model.groups.len(), 1);
        let spots = &model.groups["Küche/Spots"];
        assert_eq!(spots.members[2].brightness_for(50.0), 40.0);
        assert!(!model.lights["Küche/spot1"].exposed);
        assert!(model.lights["Küche/decke"].exposed);
        assert_eq!(model.triggers.len(), 3);

        let sun = &model.triggers[2];
        let actions = script.run(sun.proc_id, "Knut::Sun.new(20.0, 180.0, nil, nil), Time.now", &serde_json::json!({})).unwrap();
        assert_eq!(
            actions,
            vec![Action::Adapt {
                targets: vec!["Küche/Spots".into()],
                brightness: None,
                kelvin: Some(4000.0),
                transition: Some(60.0),
            }]
        );
    }

    #[test]
    fn trigger_blocks_see_state() {
        let (mut script, model) = Script::load_str(CONFIG, "config.rb").unwrap();
        let motion = &model.triggers[1];
        let args = r#"Knut::Event.new("Flur/Bewegung", :motion, {"occupancy" => true}), Time.now"#;
        let on = serde_json::json!({"Flur/Licht": {"on": true}});
        assert!(script.run(motion.proc_id, args, &on).unwrap().is_empty());
        let off = serde_json::json!({"Flur/Licht": {"on": false}});
        assert_eq!(script.run(motion.proc_id, args, &off).unwrap().len(), 1);
    }

    #[test]
    fn resolves_the_weekday_schedule() {
        let config = r#"
schedule do
  weekdays wake: "06:30", bed: "23:00"
  friday   bed: "00:30"
  saturday wake: "08:30", bed: "00:30"
  sunday   wake: "08:30", bed: "23:00"
end
"#;
        let (_, model) = Script::load_str(config, "config.rb").unwrap_or_else(|e| panic!("{e}"));
        let schedule = model.schedule.unwrap();
        let day = |wake, bed| crate::model::ScheduleDay { wake, bed };
        assert_eq!(schedule[1], day(390, 1380), "monday");
        assert_eq!(schedule[4], day(390, 1380), "thursday");
        assert_eq!(schedule[5], day(390, 1470), "friday's bedtime is saturday 00:30");
        assert_eq!(schedule[6], day(510, 1470), "saturday");
        assert_eq!(schedule[0], day(510, 1380), "sunday");

        let printed = model.to_string();
        assert!(printed.contains("friday     wake 06:30  bed 00:30 (saturday)"), "{printed}");
        assert!(printed.contains("sunday     wake 08:30  bed 23:00\n"), "{printed}");
    }

    #[test]
    fn reports_schedule_errors() {
        let error = |config: &str| Script::load_str(config, "config.rb").err().unwrap().to_string();
        let missing = error("schedule do\n  weekdays wake: \"06:30\", bed: \"23:00\"\nend\n");
        assert!(missing.starts_with("config.rb:1: schedule: sunday needs wake: and bed:"), "{missing}");
        let bad_time = error("schedule do\n  monday wake: \"6:30\"\nend\n");
        assert!(bad_time.starts_with("config.rb:2: monday wake: must be an \"HH:MM\" time"), "{bad_time}");
        let late = error("schedule do\n  monday bed: \"24:00\"\nend\n");
        assert!(late.contains("must be an \"HH:MM\" time"), "{late}");
        let unknown = error("schedule do\n  weekdays sleep: \"23:00\"\nend\n");
        assert!(unknown.starts_with("config.rb:2: unknown option(s) for weekdays: sleep"), "{unknown}");
    }

    #[test]
    fn reports_errors_with_config_lines() {
        let config = "zigbee \"z2m\", mqtt: \"mqtt://x\"\nroom \"A\" do\n  light \"l\", zigbee: \"1\"\n  group \"G\", lights: [:nope]\nend\n";
        let error = Script::load_str(config, "config.rb").err().unwrap().to_string();
        assert!(error.contains("config.rb:4"), "{error}");
        assert!(error.contains("nope"), "{error}");

        let error = Script::load_str("room \"A\" do\n  light \"l\"\nend\n", "config.rb").err().unwrap().to_string();
        assert!(error.starts_with("config.rb:2: "), "{error}");

        let error = Script::load_str("scene \"S\" do\n  turn_on \"A/b\"\nend\n", "config.rb").err().unwrap().to_string();
        assert!(error.contains("unknown light or group \"A/b\""), "{error}");
    }
}
