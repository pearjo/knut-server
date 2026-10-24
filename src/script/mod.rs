//! Loads the Ruby DSL and runs scene and trigger blocks.

pub(crate) mod mruby;

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
pub(crate) enum Action {
    Set {
        targets: Vec<String>,
        on: Option<bool>,
        brightness: Option<f64>,
        kelvin: Option<f64>,
        transition: Option<f64>,
    },
    Adapt {
        targets: Vec<String>,
        /// 0–1, before room daylight is subtracted.
        need: Option<f64>,
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
pub(crate) enum LoadError {
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
pub(crate) fn describe(error: &RubyError) -> String {
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
    let user_frames: Vec<&String> = error
        .backtrace
        .iter()
        .filter(|line| !line.starts_with(PRELUDE_FILE))
        .collect();
    let mut text = message.to_string();
    for frame in user_frames {
        text.push_str("\n    from ");
        text.push_str(frame);
    }
    text
}

/// The config's Ruby interpreter, kept alive to run scene and trigger blocks.
pub(crate) struct Script {
    ruby: Mruby,
}

impl Script {
    /// Loads and validates the config at `path`.
    pub(crate) fn load(path: &Path) -> Result<(Self, Model), LoadError> {
        let code = std::fs::read_to_string(path).map_err(LoadError::Io)?;
        let filename = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into(),
        );
        Self::load_str(&code, &filename)
    }

    pub(crate) fn load_str(code: &str, filename: &str) -> Result<(Self, Model), LoadError> {
        let mut ruby = Mruby::new();
        ruby.eval(PRELUDE, PRELUDE_FILE).map_err(LoadError::Ruby)?;
        ruby.eval(code, filename).map_err(LoadError::Ruby)?;

        let json = ruby
            .eval("Knut.model_json", PRELUDE_FILE)
            .map_err(LoadError::Ruby)?;
        // The prelude checks shapes; what is left are value errors such as a bad IP address.
        let raw: RawModel = serde_json::from_str(&json).map_err(|error| {
            LoadError::Invalid(ValidationErrors(vec![format!("{filename}: {error}")]))
        })?;

        let mut tables = HashMap::new();
        for id in raw.mapping_procs() {
            let json = ruby
                .eval(&format!("Knut.mapping_table({id})"), PRELUDE_FILE)
                .map_err(LoadError::Ruby)?;
            let table: Vec<f64> =
                serde_json::from_str(&json).expect("the prelude emits a JSON array");
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
    pub(crate) fn run(
        &mut self,
        proc_id: usize,
        args: &str,
        state: &serde_json::Value,
    ) -> Result<Vec<Action>, RubyError> {
        let code = format!(
            "Knut.run({proc_id}, [{args}], {})",
            mruby::ruby_literal(state)
        );
        let json = self.ruby.eval(&code, PRELUDE_FILE)?;
        Ok(serde_json::from_str(&json).expect("the prelude emits valid action JSON"))
    }

    /// Runs the staging block `proc_id` for `need` (0–1) at the Unix time
    /// `now` and returns the names of the lights it picked.
    pub(crate) fn staging(
        &mut self,
        proc_id: usize,
        need: f64,
        now: i64,
    ) -> Result<Vec<String>, RubyError> {
        let json = self.ruby.eval(
            &format!("Knut.staging({proc_id}, {need:?}, {now})"),
            PRELUDE_FILE,
        )?;
        Ok(serde_json::from_str(&json).expect("the prelude emits a JSON array"))
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
                Err(error) => errors.push(format!(
                    "{}: scene {:?}: {}",
                    scene.line,
                    scene.name,
                    describe(&error)
                )),
            }
        }
        errors
    }
}

/// Unknown targets and scenes in a list of actions.
pub(crate) fn validate_actions(model: &Model, actions: &[Action]) -> Vec<String> {
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
    use crate::model::Staging;
    use chrono::TimeZone;

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
        let (mut script, model) =
            Script::load_str(CONFIG, "config.rb").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(model.groups.len(), 1);
        let spots = &model.groups["Küche/Spots"];
        assert_eq!(spots.members[2].brightness_for(50.0), 40.0);
        assert!(!model.lights["Küche/spot1"].exposed);
        assert!(model.lights["Küche/decke"].exposed);
        assert_eq!(model.triggers.len(), 3);

        let sun = &model.triggers[2];
        let actions = script
            .run(
                sun.proc_id,
                "Knut::Sun.new(20.0, 180.0, nil, nil), Time.now",
                &serde_json::json!({}),
            )
            .unwrap();
        assert_eq!(
            actions,
            vec![Action::Adapt {
                targets: vec!["Küche/Spots".into()],
                need: None,
                kelvin: Some(4000.0),
                transition: Some(60.0),
            }]
        );
    }

    #[test]
    fn the_example_config_adapts_by_need_and_warmth() {
        let (mut script, model) =
            Script::load_str(include_str!("../../examples/config.rb"), "config.rb")
                .unwrap_or_else(|e| panic!("{e}"));
        let sun = model
            .triggers
            .iter()
            .find(|trigger| trigger.kind == crate::model::TriggerKind::SunTick)
            .unwrap();
        let noon = chrono::Local
            .with_ymd_and_hms(2026, 10, 22, 12, 0, 0)
            .unwrap()
            .timestamp();
        let args = format!("Knut::Sun.new(20.0, 180.0, nil, nil), Time.at({noon})");
        let actions = script
            .run(sun.proc_id, &args, &serde_json::json!({}))
            .unwrap();
        let [Action::Adapt {
            targets,
            need: Some(need),
            kelvin: Some(kelvin),
            ..
        }] = actions.as_slice()
        else {
            panic!("{actions:?}");
        };
        assert_eq!(targets, &["Kitchen/Spots", "Living Room/Light"]);
        assert_eq!(*need, 1.0, "full demand at noon");
        assert!((3000.0..=4000.0).contains(kelvin), "{kelvin}");
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
        assert_eq!(
            schedule[5],
            day(390, 1470),
            "friday's bedtime is saturday 00:30"
        );
        assert_eq!(schedule[6], day(510, 1470), "saturday");
        assert_eq!(schedule[0], day(510, 1380), "sunday");

        let printed = model.to_string();
        assert!(
            printed.contains("friday     wake 06:30  bed 00:30 (saturday)"),
            "{printed}"
        );
        assert!(
            printed.contains("sunday     wake 08:30  bed 23:00\n"),
            "{printed}"
        );
    }

    #[test]
    fn reports_schedule_errors() {
        let error = |config: &str| {
            Script::load_str(config, "config.rb")
                .err()
                .unwrap()
                .to_string()
        };
        let missing = error("schedule do\n  weekdays wake: \"06:30\", bed: \"23:00\"\nend\n");
        assert!(
            missing.starts_with("config.rb:1: schedule: sunday needs wake: and bed:"),
            "{missing}"
        );
        let bad_time = error("schedule do\n  monday wake: \"6:30\"\nend\n");
        assert!(
            bad_time.starts_with("config.rb:2: monday wake: must be an \"HH:MM\" time"),
            "{bad_time}"
        );
        let late = error("schedule do\n  monday bed: \"24:00\"\nend\n");
        assert!(late.contains("must be an \"HH:MM\" time"), "{late}");
        let unknown = error("schedule do\n  weekdays sleep: \"23:00\"\nend\n");
        assert!(
            unknown.starts_with("config.rb:2: unknown option(s) for weekdays: sleep"),
            "{unknown}"
        );
    }

    const CURVE_CONFIG: &str = r#"
schedule do
  weekdays wake: "06:30", bed: "23:00"
  friday   bed: "00:30"
  saturday wake: "08:30", bed: "00:30"
  sunday   wake: "08:30", bed: "23:00"
end
$demand = curve(
  [:bed + 1.h,   0.0],
  [:wake,        0.0],
  [:wake + 30.m, 1.0],
  ["17:00",      1.0],
  [:bed - 3.h,   0.6],
  [:bed - 1.h,   0.25])
"#;

    /// `$demand` from [`CURVE_CONFIG`] at local time "YYYY-MM-DD HH:MM".
    fn demand_at(script: &mut Script, time: &str) -> f64 {
        let (date, clock) = time.split_once(' ').unwrap();
        // Parsed so that "08" doesn't reach Ruby as an octal literal.
        let number = |text: &str| text.parse::<u32>().unwrap();
        let [year, month, day]: [u32; 3] = date
            .split('-')
            .map(number)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let (hour, minute) = clock
            .split_once(':')
            .map(|(h, m)| (number(h), number(m)))
            .unwrap();
        let code = format!("$demand.at(Time.local({year}, {month}, {day}, {hour}, {minute}))");
        script.ruby.eval(&code, "test").unwrap().parse().unwrap()
    }

    fn assert_near(actual: f64, expected: f64, what: &str) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "{what}: {actual}, expected {expected}"
        );
    }

    #[test]
    fn curves_hit_keyframes_and_ease_between_them() {
        let (mut script, _) =
            Script::load_str(CURVE_CONFIG, "config.rb").unwrap_or_else(|e| panic!("{e}"));
        // 2026-10-22 is a Thursday.
        for (time, expected) in [
            ("2026-10-22 06:30", 0.0),
            ("2026-10-22 07:00", 1.0),
            ("2026-10-22 17:00", 1.0),
            ("2026-10-22 20:00", 0.6),
            ("2026-10-22 22:00", 0.25),
            ("2026-10-23 00:00", 0.0),
            ("2026-10-22 03:00", 0.0),
            ("2026-10-22 12:00", 1.0),
            ("2026-10-22 06:45", 0.5),
        ] {
            assert_near(demand_at(&mut script, time), expected, time);
        }
    }

    #[test]
    fn curve_anchors_move_with_the_schedule() {
        let (mut script, _) = Script::load_str(CURVE_CONFIG, "config.rb").unwrap();
        // Friday's bedtime is Saturday 00:30: at midnight the evening ramp from
        // 23:30 (0.25) to 01:30 (0.0) is a quarter done.
        let eased = 0.25 * (1.0 + (std::f64::consts::PI / 4.0).cos()) / 2.0;
        assert_near(
            demand_at(&mut script, "2026-10-24 00:00"),
            eased,
            "Friday night, past midnight",
        );
        assert_near(
            demand_at(&mut script, "2026-10-24 01:30"),
            0.0,
            "Friday's bed + 1 h",
        );
        assert_near(
            demand_at(&mut script, "2026-10-24 08:30"),
            0.0,
            "Saturday's wake",
        );
        assert_near(
            demand_at(&mut script, "2026-10-24 09:00"),
            1.0,
            "Saturday's wake + 30 min",
        );
        assert_near(
            demand_at(&mut script, "2026-10-23 07:00"),
            1.0,
            "Friday's wake + 30 min",
        );
    }

    #[test]
    fn curves_stay_between_neighbouring_keyframes() {
        let (mut script, _) = Script::load_str(CURVE_CONFIG, "config.rb").unwrap();
        // Thursday 22:00 (0.25) to Friday 00:00 (0.0), every 5 minutes.
        let mut previous = f64::INFINITY;
        for step in 0..=24 {
            let minutes = 22 * 60 + step * 5;
            let time = if minutes < 1440 {
                format!("2026-10-22 {:02}:{:02}", minutes / 60, minutes % 60)
            } else {
                format!(
                    "2026-10-23 {:02}:{:02}",
                    (minutes - 1440) / 60,
                    minutes % 60
                )
            };
            let value = demand_at(&mut script, &time);
            assert!(
                (0.0..=0.25).contains(&value) && value <= previous,
                "{time}: {value} after {previous}"
            );
            previous = value;
        }
    }

    #[test]
    fn reports_curve_errors() {
        let error = |config: &str| {
            Script::load_str(config, "config.rb")
                .err()
                .unwrap()
                .to_string()
        };
        let crossing = error(
            "schedule do\n  weekdays wake: \"06:30\", bed: \"23:00\"\n  monday bed: \"19:00\"\n  saturday wake: \"08:30\", bed: \"23:00\"\n  sunday wake: \"08:30\", bed: \"23:00\"\nend\ncurve([:wake, 0], [\"17:00\", 1], [:bed - 3.h, 0.5])\n",
        );
        assert!(crossing.starts_with("config.rb:7: curve keyframes on monday are not in clock order: 06:30, 17:00, 16:00"), "{crossing}");
        let unscheduled = error("curve([:wake, 0], [\"17:00\", 1])\n");
        assert!(
            unscheduled.starts_with("config.rb:1: curves with :wake or :bed need a schedule"),
            "{unscheduled}"
        );
        let anchor = error("curve([:noon + 1.h, 0], [\"17:00\", 1])\n");
        assert!(
            anchor.starts_with("config.rb:1: only :wake and :bed"),
            "{anchor}"
        );
        let bad_time = error("curve([\"7:00\", 0], [\"17:00\", 1])\n");
        assert!(
            bad_time.starts_with("config.rb:1: keyframe time must be an \"HH:MM\" time"),
            "{bad_time}"
        );
    }

    #[test]
    fn reads_room_daylight() {
        let config = "room \"Küche\" do\n  daylight 0.8, facing: :east\nend\nroom \"Wohnzimmer\" do\n  daylight 0.4, facing: [:south, :west]\nend\nroom \"Flur\" do\nend\n";
        let (_, model) = Script::load_str(config, "config.rb").unwrap_or_else(|e| panic!("{e}"));
        let daylight = |room: usize| model.rooms[room].daylight.clone();
        assert_eq!(
            daylight(0),
            Some(crate::model::Daylight {
                amount: 0.8,
                facing: vec![90.0]
            })
        );
        assert_eq!(
            daylight(1),
            Some(crate::model::Daylight {
                amount: 0.4,
                facing: vec![180.0, 270.0]
            })
        );
        assert_eq!(daylight(2), None);
        assert!(
            model
                .to_string()
                .contains("room Wohnzimmer  daylight 0.4 facing south, west\n"),
            "{model}"
        );

        let error = |config: &str| {
            Script::load_str(config, "config.rb")
                .err()
                .unwrap()
                .to_string()
        };
        let too_much = error("room \"A\" do\n  daylight 1.5, facing: :east\nend\n");
        assert!(
            too_much.starts_with("config.rb:2: daylight must be between 0 and 1"),
            "{too_much}"
        );
        let unknown = error("room \"A\" do\n  daylight 0.5, facing: [:east, :up]\nend\n");
        assert!(
            unknown.starts_with("config.rb:2: unknown direction(s) for facing: :up"),
            "{unknown}"
        );
        let twice = error(
            "room \"A\" do\n  daylight 0.5, facing: :east\n  daylight 0.5, facing: :west\nend\n",
        );
        assert!(
            twice.starts_with("config.rb:3: daylight is already set"),
            "{twice}"
        );
    }

    #[test]
    fn adapt_points_brightness_users_to_need() {
        let config = "on \"Clock\", at: \"06:30\" do\n  adapt all_groups, brightness: 50\nend\n";
        let (mut script, model) = Script::load_str(config, "config.rb").unwrap();
        let error = script
            .run(
                model.triggers[0].proc_id,
                "Time.now",
                &serde_json::json!({}),
            )
            .unwrap_err();
        assert_eq!(
            describe(&error),
            "adapt takes need: (0–1) instead of brightness:"
        );
    }

    #[test]
    fn reads_and_checks_stages() {
        let group = |stages: &str| {
            format!(
                "zigbee \"z2m\", mqtt: \"mqtt://x\"\nroom \"A\" do\n  light \"a\", zigbee: \"1\"\n  light \"b\", zigbee: \"2\"\n  light \"c\", zigbee: \"3\"\n  group \"G\", lights: [:a, :b, :c] do\n    {stages}\n  end\nend\n"
            )
        };
        let (_, model) = Script::load_str(&group("stages [:b], [:a, :c]"), "config.rb")
            .unwrap_or_else(|e| panic!("{e}"));
        let paths = |names: &[&str]| names.iter().map(|n| format!("A/{n}")).collect::<Vec<_>>();
        assert_eq!(
            model.groups["A/G"].staging,
            Some(Staging::Stages(vec![paths(&["b"]), paths(&["a", "c"])]))
        );
        assert!(
            model.to_string().contains("    stages b | a, c\n"),
            "{model}"
        );

        let error = |stages: &str| {
            Script::load_str(&group(stages), "config.rb")
                .err()
                .unwrap()
                .to_string()
        };
        let missing = error("stages [:b], [:a]");
        assert!(
            missing.starts_with("config.rb:7: stages must list \"c\" exactly once"),
            "{missing}"
        );
        let twice = error("stages [:b], [:a, :b, :c]");
        assert!(
            twice.contains("config.rb:7: stages must list \"b\" exactly once"),
            "{twice}"
        );
        let stranger = error("stages [:b], [:a, :c, :d]");
        assert!(
            stranger.contains(
                "config.rb:7: stages refer to \"d\", which is not a light of group \"G\""
            ),
            "{stranger}"
        );
        let flat = error("stages :b, :a");
        assert!(
            flat.starts_with("config.rb:7: each stage must be a non-empty Array"),
            "{flat}"
        );
        let both = error("stages [:b], [:a, :c]\n    staging { |need, now| [:b] }");
        assert!(
            both.starts_with("config.rb:8: a group takes either stages or staging, not both"),
            "{both}"
        );
        let both = error("staging { |need, now| [:b] }\n    stages [:b], [:a, :c]");
        assert!(
            both.starts_with("config.rb:8: a group takes either stages or staging, not both"),
            "{both}"
        );
    }

    #[test]
    fn staging_blocks_return_light_names() {
        let config = "zigbee \"z2m\", mqtt: \"mqtt://x\"\nroom \"A\" do\n  light \"a\", zigbee: \"1\"\n  light \"b\", zigbee: \"2\"\n  group \"G\", lights: [:a, :b] do\n    staging { |need, now| now.hour < 9 || need > 0.5 ? [:a, :b] : [:b] }\n  end\n  group \"H\", lights: [:a, :b] do\n    staging { |need, now| :b }\n  end\nend\n";
        let (mut script, model) =
            Script::load_str(config, "config.rb").unwrap_or_else(|e| panic!("{e}"));
        let proc_of = |group: &str| match &model.groups[group].staging {
            Some(Staging::Custom { proc_id, .. }) => *proc_id,
            other => panic!("{other:?}"),
        };
        assert!(
            model
                .to_string()
                .contains("    staging block at config.rb:6\n"),
            "{model}"
        );

        let at = |hour| {
            chrono::Local
                .with_ymd_and_hms(2026, 10, 22, hour, 0, 0)
                .unwrap()
                .timestamp()
        };
        assert_eq!(
            script.staging(proc_of("A/G"), 0.2, at(8)).unwrap(),
            ["a", "b"],
            "early morning"
        );
        assert_eq!(
            script.staging(proc_of("A/G"), 0.2, at(12)).unwrap(),
            ["b"],
            "low need at noon"
        );
        assert_eq!(
            script.staging(proc_of("A/G"), 0.8, at(12)).unwrap(),
            ["a", "b"],
            "high need at noon"
        );

        let error = script.staging(proc_of("A/H"), 0.5, at(12)).unwrap_err();
        assert_eq!(
            describe(&error),
            "staging block returned :b, expected an Array of lights like [:spot1]"
        );
    }

    #[test]
    fn reports_errors_with_config_lines() {
        let config = "zigbee \"z2m\", mqtt: \"mqtt://x\"\nroom \"A\" do\n  light \"l\", zigbee: \"1\"\n  group \"G\", lights: [:nope]\nend\n";
        let error = Script::load_str(config, "config.rb")
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("config.rb:4"), "{error}");
        assert!(error.contains("nope"), "{error}");

        let error = Script::load_str("room \"A\" do\n  light \"l\"\nend\n", "config.rb")
            .err()
            .unwrap()
            .to_string();
        assert!(error.starts_with("config.rb:2: "), "{error}");

        let error = Script::load_str("scene \"S\" do\n  turn_on \"A/b\"\nend\n", "config.rb")
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("unknown light or group \"A/b\""), "{error}");
    }
}
