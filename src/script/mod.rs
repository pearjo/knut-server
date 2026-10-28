//! Loads the Ruby DSL and runs scene and trigger blocks.

mod mruby;

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use chrono::{DateTime, Local};
use serde::Deserialize;

use crate::model::{Model, RawModel, Scene, ValidationErrors};
use crate::sun::{SunPosition, SunTimes};
use mruby::Mruby;
pub(crate) use mruby::RubyError;

/// The Ruby side of the DSL, in load order: internals first, then the
/// runtime Rust calls, then the configuration language itself.
const PRELUDE: [(&str, &str); 6] = [
    ("(knut)/internal.rb", include_str!("prelude/internal.rb")),
    ("(knut)/json.rb", include_str!("prelude/json.rb")),
    ("(knut)/runtime.rb", include_str!("prelude/runtime.rb")),
    ("(knut)/objects.rb", include_str!("prelude/objects.rb")),
    ("(knut)/builders.rb", include_str!("prelude/builders.rb")),
    ("(knut)/dsl.rb", include_str!("prelude/dsl.rb")),
];
/// Every prelude file name starts with this, so that errors and backtraces
/// can tell the prelude from the config.
const PRELUDE_FILE: &str = "(knut)";

fn load_prelude(ruby: &mut Mruby) -> Result<(), RubyError> {
    for (filename, code) in PRELUDE {
        ruby.eval(code, filename)?;
    }
    Ok(())
}

/// What a scene or trigger block receives: `|sun, now|`, `|now|` or
/// `|event, now|`.
#[derive(Debug, Clone)]
pub(crate) enum BlockArgs {
    /// Scenes take no arguments.
    None,
    Clock {
        now: DateTime<Local>,
    },
    Sun {
        position: SunPosition,
        /// Today's sunrise and sunset; `None` during polar day or night.
        times: Option<SunTimes>,
        now: DateTime<Local>,
    },
    Device {
        path: String,
        event: String,
        /// The device's report.
        data: serde_json::Value,
        now: DateTime<Local>,
    },
}

impl BlockArgs {
    /// The arguments as Ruby expressions, separated by commas.
    fn to_ruby(&self) -> String {
        match self {
            Self::None => String::new(),
            Self::Clock { now } => ruby_time(now),
            Self::Sun {
                position,
                times,
                now,
            } => format!("{}, {}", ruby_sun(position, times.as_ref()), ruby_time(now)),
            Self::Device {
                path,
                event,
                data,
                now,
            } => format!(
                "Knut::Event.new({}, {}, {}), {}",
                mruby::ruby_string(path),
                mruby::ruby_string(event),
                mruby::ruby_literal(data),
                ruby_time(now)
            ),
        }
    }
}

fn ruby_time(time: &DateTime<Local>) -> String {
    format!("Time.at({})", time.timestamp())
}

/// A `Knut::Sun`; it takes sunrise and sunset as Unix seconds, or `nil`
/// during polar day or night, and makes them Time itself.
fn ruby_sun(position: &SunPosition, times: Option<&SunTimes>) -> String {
    let unix_or_nil = |time: Option<DateTime<chrono::Utc>>| {
        time.map_or("nil".to_string(), |time| time.timestamp().to_string())
    };
    let sunrise = unix_or_nil(times.map(|times| times.sunrise));
    let sunset = unix_or_nil(times.map(|times| times.sunset));
    format!(
        "Knut::Sun.new({:.4}, {:.4}, {sunrise}, {sunset})",
        position.elevation, position.azimuth
    )
}

/// What `set`, `turn_on` or `turn_off` asks of each target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub(crate) struct SetValues {
    pub on: Option<bool>,
    /// 1–100.
    pub brightness: Option<f64>,
    pub kelvin: Option<f64>,
    /// Seconds.
    pub transition: Option<f64>,
}

/// What `adapt` asks of each target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub(crate) struct AdaptValues {
    /// 0–1, before room daylight is subtracted.
    pub need: Option<f64>,
    pub kelvin: Option<f64>,
    /// Seconds.
    pub transition: Option<f64>,
}

/// Something a scene or trigger block asked for.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub(crate) enum Action {
    /// `set`, `turn_on` and `turn_off`.
    Set {
        targets: Vec<String>,
        #[serde(flatten)]
        values: SetValues,
    },
    Adapt {
        targets: Vec<String>,
        #[serde(flatten)]
        values: AdaptValues,
    },
    Activate {
        scene: String,
    },
    Log {
        message: String,
    },
}

/// Why a config did not load.
#[derive(Debug)]
pub(crate) enum LoadError {
    /// The file could not be read.
    Io(std::io::Error),
    /// The Ruby code raised, including the prelude's own config errors.
    Ruby(RubyError),
    /// The config ran, but describes a home that does not fit together.
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
    let message = without_prelude_location(&error.message);
    if let Some(config_error) = message.strip_suffix(" (Knut::ConfigError)") {
        return config_error.to_string();
    }

    let config_frames = error
        .backtrace
        .iter()
        .filter(|line| !line.starts_with(PRELUDE_FILE));
    let mut text = message.to_string();
    for frame in config_frames {
        text.push_str("\n    from ");
        text.push_str(frame);
    }
    text
}

/// The message without a leading "(knut)/file.rb:12: ".
fn without_prelude_location(message: &str) -> &str {
    let Some(rest) = message.strip_prefix(PRELUDE_FILE) else {
        return message;
    };
    rest.split_once(": ").map_or(rest, |(_, message)| message)
}

/// The config's Ruby interpreter, kept alive to run scene and trigger blocks.
pub(crate) struct Script {
    ruby: Mruby,
}

impl Script {
    /// Loads and validates the config at `path`. Error messages name the
    /// config by its file name, e.g. `config.rb:12`.
    pub(crate) fn load(path: &Path) -> Result<(Self, Model), LoadError> {
        let code = std::fs::read_to_string(path).map_err(LoadError::Io)?;
        let filename = match path.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => path.display().to_string(),
        };
        Self::load_str(&code, &filename)
    }

    /// Loads and validates a config given as source code; `filename` names
    /// it in error messages.
    pub(crate) fn load_str(code: &str, filename: &str) -> Result<(Self, Model), LoadError> {
        let mut ruby = Mruby::new();
        load_prelude(&mut ruby).map_err(LoadError::Ruby)?;
        ruby.eval(code, filename).map_err(LoadError::Ruby)?;

        let raw = recorded_model(&mut ruby, filename)?;
        let tables = mapping_tables(&mut ruby, &raw)?;
        let model = raw.build(&tables).map_err(LoadError::Invalid)?;

        let mut script = Self { ruby };
        let errors = script.dry_run_scenes(&model);
        if !errors.is_empty() {
            return Err(LoadError::Invalid(ValidationErrors(errors)));
        }
        Ok((script, model))
    }

    /// Runs the scene or trigger block `proc_id` with `args` and returns the
    /// actions it requested. `state` is the entity snapshot exposed through
    /// `state(path)` and `on?(path)`.
    pub(crate) fn run(
        &mut self,
        proc_id: usize,
        args: &BlockArgs,
        state: &serde_json::Value,
    ) -> Result<Vec<Action>, RubyError> {
        let code = format!(
            "Knut.run({proc_id}, [{}], {})",
            args.to_ruby(),
            mruby::ruby_literal(state)
        );
        let json = self.ruby.eval(&code, PRELUDE_FILE)?;
        let actions = serde_json::from_str(&json).expect("the prelude emits valid action JSON");
        Ok(actions)
    }

    /// Runs the staging block `proc_id` for `need` (0–1) at the Unix time
    /// `now` and returns the names of the lights it picked.
    pub(crate) fn staging(
        &mut self,
        proc_id: usize,
        need: f64,
        now: i64,
    ) -> Result<Vec<String>, RubyError> {
        let code = format!("Knut.staging({proc_id}, {need:?}, {now})");
        let json = self.ruby.eval(&code, PRELUDE_FILE)?;
        let names = serde_json::from_str(&json).expect("the prelude emits a JSON array");
        Ok(names)
    }

    /// Scenes are almost always static, so running them once at load time
    /// catches typos in their targets before the daemon starts.
    fn dry_run_scenes(&mut self, model: &Model) -> Vec<String> {
        let mut errors = Vec::new();
        for scene in &model.scenes {
            let problems = self.scene_problems(model, scene);
            for problem in problems {
                errors.push(format!("{}: scene {:?}: {problem}", scene.line, scene.name));
            }
        }
        errors
    }

    /// What is wrong with the actions `scene` asks for, or why it fails.
    fn scene_problems(&mut self, model: &Model, scene: &Scene) -> Vec<String> {
        let no_state = serde_json::json!({});
        match self.run(scene.proc_id, &BlockArgs::None, &no_state) {
            Ok(actions) => validate_actions(model, &actions),
            Err(error) => vec![describe(&error)],
        }
    }
}

/// The model the prelude recorded while the config loaded.
fn recorded_model(ruby: &mut Mruby, filename: &str) -> Result<RawModel, LoadError> {
    let json = ruby
        .eval("Knut.model_json", PRELUDE_FILE)
        .map_err(LoadError::Ruby)?;
    // The prelude checks shapes; what is left are value errors such as a bad IP address.
    serde_json::from_str(&json).map_err(|error| {
        let message = format!("{filename}: {error}");
        LoadError::Invalid(ValidationErrors(vec![message]))
    })
}

/// Every brightness mapping evaluated for 1..100, keyed by proc id.
fn mapping_tables(ruby: &mut Mruby, raw: &RawModel) -> Result<HashMap<usize, Vec<f64>>, LoadError> {
    let mut tables = HashMap::new();
    for id in raw.mapping_procs() {
        let json = ruby
            .eval(&format!("Knut.mapping_table({id})"), PRELUDE_FILE)
            .map_err(LoadError::Ruby)?;
        let table: Vec<f64> = serde_json::from_str(&json).expect("the prelude emits a JSON array");
        tables.insert(id, table);
    }
    Ok(tables)
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

    const HOME: &str = include_str!("../../tests/fixtures/home.rb");

    fn load(config: &str) -> (Script, Model) {
        Script::load_str(config, "config.rb").unwrap_or_else(|error| panic!("{error}"))
    }

    /// The message a config that fails to load reports.
    fn load_error(config: &str) -> String {
        match Script::load_str(config, "config.rb") {
            Ok(_) => panic!("the config loaded:\n{config}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn loads_rooms_groups_and_triggers() {
        let (mut script, model) = load(HOME);
        assert_eq!(model.groups.len(), 1);
        let spots = &model.groups["Kitchen/Spots"];
        assert_eq!(spots.members[2].brightness_for(50.0), 40.0);
        assert!(
            !model.lights["Kitchen/spot1"].exposed,
            "grouped lights are hidden"
        );
        assert!(model.lights["Kitchen/ceiling"].exposed);
        assert_eq!(model.triggers.len(), 3);

        let sun = &model.triggers[2];
        let actions = script
            .run(
                sun.proc_id,
                &sun_at(20.0, chrono::Local::now()),
                &serde_json::json!({}),
            )
            .unwrap();
        assert_eq!(
            actions,
            vec![Action::Adapt {
                targets: vec!["Kitchen/Spots".into()],
                values: AdaptValues {
                    need: None,
                    kelvin: Some(4000.0),
                    transition: Some(60.0),
                },
            }]
        );
    }

    #[test]
    fn the_example_config_adapts_by_need_and_warmth() {
        let (mut script, model) = load(include_str!("../../examples/config.rb"));
        let sun = model
            .triggers
            .iter()
            .find(|trigger| trigger.kind == crate::model::TriggerKind::SunTick)
            .unwrap();
        let noon = chrono::Local
            .with_ymd_and_hms(2026, 10, 22, 12, 0, 0)
            .unwrap();
        let actions = script
            .run(sun.proc_id, &sun_at(20.0, noon), &serde_json::json!({}))
            .unwrap();
        let [Action::Adapt {
            targets,
            values:
                AdaptValues {
                    need: Some(need),
                    kelvin: Some(kelvin),
                    ..
                },
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
        let (mut script, model) = load(HOME);
        let motion = &model.triggers[1];
        let args = BlockArgs::Device {
            path: "Hallway/Motion".into(),
            event: "motion".into(),
            data: serde_json::json!({"occupancy": true}),
            now: chrono::Local::now(),
        };
        let on = serde_json::json!({"Hallway/Light": {"on": true}});
        assert!(script.run(motion.proc_id, &args, &on).unwrap().is_empty());
        let off = serde_json::json!({"Hallway/Light": {"on": false}});
        assert_eq!(script.run(motion.proc_id, &args, &off).unwrap().len(), 1);
    }

    fn sun_at(elevation: f64, now: chrono::DateTime<chrono::Local>) -> BlockArgs {
        BlockArgs::Sun {
            position: SunPosition {
                elevation,
                azimuth: 180.0,
            },
            times: None,
            now,
        }
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
        let (_, model) = load(config);
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
        let missing = load_error(
            r#"schedule do
  weekdays wake: "06:30", bed: "23:00"
end"#,
        );
        assert!(
            missing.starts_with("config.rb:1: schedule: sunday needs wake: and bed:"),
            "{missing}"
        );
        let bad_time = load_error(
            r#"schedule do
  monday wake: "6:30"
end"#,
        );
        assert!(
            bad_time.starts_with("config.rb:2: monday wake: must be an \"HH:MM\" time"),
            "{bad_time}"
        );
        let late = load_error(
            r#"schedule do
  monday bed: "24:00"
end"#,
        );
        assert!(late.contains("must be an \"HH:MM\" time"), "{late}");
        let unknown = load_error(
            r#"schedule do
  weekdays sleep: "23:00"
end"#,
        );
        assert!(
            unknown.starts_with("config.rb:2: unknown option(s) for weekdays: sleep"),
            "{unknown}"
        );
    }

    const CURVES: &str = include_str!("../../tests/fixtures/curves.rb");

    /// `$demand` from [`CURVES`] at local time "YYYY-MM-DD HH:MM".
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
        let (mut script, _) = load(CURVES);
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
        let (mut script, _) = load(CURVES);
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
        let (mut script, _) = load(CURVES);
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
        // Monday's early bedtime puts :bed - 3.h before 17:00.
        let crossing = load_error(
            r#"schedule do
  weekdays wake: "06:30", bed: "23:00"
  monday   bed: "19:00"
  saturday wake: "08:30", bed: "23:00"
  sunday   wake: "08:30", bed: "23:00"
end
curve([:wake, 0], ["17:00", 1], [:bed - 3.h, 0.5])"#,
        );
        assert!(
            crossing.starts_with(
                "config.rb:7: curve keyframes on monday are not in clock order: 06:30, 17:00, 16:00"
            ),
            "{crossing}"
        );
        let unscheduled = load_error(r#"curve([:wake, 0], ["17:00", 1])"#);
        assert!(
            unscheduled.starts_with("config.rb:1: curves with :wake or :bed need a schedule"),
            "{unscheduled}"
        );
        let anchor = load_error(r#"curve([:noon + 1.h, 0], ["17:00", 1])"#);
        assert!(
            anchor.starts_with("config.rb:1: only :wake and :bed"),
            "{anchor}"
        );
        let bad_time = load_error(r#"curve(["7:00", 0], ["17:00", 1])"#);
        assert!(
            bad_time.starts_with("config.rb:1: keyframe time must be an \"HH:MM\" time"),
            "{bad_time}"
        );
    }

    #[test]
    fn reads_room_daylight() {
        let (_, model) = load(
            r#"room "Kitchen" do
  daylight 0.8, facing: :east
end
room "Living Room" do
  daylight 0.4, facing: [:south, :west]
end
room "Hallway" do
end"#,
        );
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
                .contains("room Living Room  daylight 0.4 facing south, west\n"),
            "{model}"
        );

        let too_much = load_error(
            r#"room "A" do
  daylight 1.5, facing: :east
end"#,
        );
        assert!(
            too_much.starts_with("config.rb:2: daylight must be between 0 and 1"),
            "{too_much}"
        );
        let unknown = load_error(
            r#"room "A" do
  daylight 0.5, facing: [:east, :up]
end"#,
        );
        assert!(
            unknown.starts_with("config.rb:2: unknown direction(s) for facing: :up"),
            "{unknown}"
        );
        let twice = load_error(
            r#"room "A" do
  daylight 0.5, facing: :east
  daylight 0.5, facing: :west
end"#,
        );
        assert!(
            twice.starts_with("config.rb:3: daylight is already set"),
            "{twice}"
        );
    }

    #[test]
    fn adapt_points_brightness_users_to_need() {
        let (mut script, model) = load(
            r#"on "Clock", at: "06:30" do
  adapt all_groups, brightness: 50
end"#,
        );
        let error = script
            .run(
                model.triggers[0].proc_id,
                &BlockArgs::Clock {
                    now: chrono::Local::now(),
                },
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
        // `stages` lands on line 7.
        let group = |stages: &str| {
            format!(
                r#"zigbee "z2m", mqtt: "mqtt://x"
room "A" do
  light "a", zigbee: "1"
  light "b", zigbee: "2"
  light "c", zigbee: "3"
  group "G", lights: [:a, :b, :c] do
    {stages}
  end
end"#
            )
        };
        let (_, model) = load(&group("stages [:b], [:a, :c]"));
        let paths = |names: &[&str]| names.iter().map(|n| format!("A/{n}")).collect::<Vec<_>>();
        assert_eq!(
            model.groups["A/G"].staging,
            Some(Staging::Stages(vec![paths(&["b"]), paths(&["a", "c"])]))
        );
        assert!(
            model.to_string().contains("    stages b | a, c\n"),
            "{model}"
        );

        let error = |stages: &str| load_error(&group(stages));
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
        let (mut script, model) = load(include_str!("../../tests/fixtures/staging_blocks.rb"));
        let proc_of = |group: &str| match &model.groups[group].staging {
            Some(Staging::Custom { proc_id, .. }) => *proc_id,
            other => panic!("{other:?}"),
        };
        assert!(
            model
                .to_string()
                .contains("    staging block at config.rb:11\n"),
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
        let unknown_light = load_error(
            r#"zigbee "z2m", mqtt: "mqtt://x"
room "A" do
  light "l", zigbee: "1"
  group "G", lights: [:nope]
end"#,
        );
        assert!(unknown_light.contains("config.rb:4"), "{unknown_light}");
        assert!(unknown_light.contains("nope"), "{unknown_light}");

        let no_device = load_error(
            r#"room "A" do
  light "l"
end"#,
        );
        assert!(no_device.starts_with("config.rb:2: "), "{no_device}");

        let unknown_target = load_error(
            r#"scene "S" do
  turn_on "A/b"
end"#,
        );
        assert!(
            unknown_target.contains("unknown light or group \"A/b\""),
            "{unknown_target}"
        );
    }

    /// Every method a config can call has to be described in the DSL
    /// reference, so the reference cannot fall behind the prelude.
    #[test]
    fn the_dsl_reference_covers_every_dsl_method() {
        let reference = include_str!("../../docs/dsl.md");
        let undocumented: Vec<String> = dsl_methods()
            .into_iter()
            .filter(|method| !mentions(reference, method))
            .collect();
        assert!(
            undocumented.is_empty(),
            "docs/dsl.md does not describe {undocumented:?}"
        );
    }

    /// The methods the prelude adds to the top level, plus the public
    /// methods of the objects a config works with.
    fn dsl_methods() -> Vec<String> {
        const PUBLIC_METHODS: &str = "[Knut::DSL, Knut::RoomBuilder, Knut::GroupBuilder, \
            Knut::ScheduleBuilder, Knut::Curve, Knut::Sun, Knut::Event, Knut::EntityState]\
            .flat_map { |object| object.public_instance_methods(false) }\
            .map(&:to_s).sort.join(' ')";
        // Ruby's own hooks, not part of the language.
        const HOOKS: [&str; 4] = [
            "initialize",
            "inspect",
            "method_missing",
            "respond_to_missing?",
        ];

        let mut ruby = Mruby::new();
        load_prelude(&mut ruby).unwrap();
        let methods = ruby.eval(PUBLIC_METHODS, "test.rb").unwrap();

        methods
            .split(' ')
            .filter(|method| !HOOKS.contains(method))
            // Operators such as `[]` are documented by example (`event[:key]`).
            .filter(|method| method.starts_with(|c: char| c.is_alphabetic()))
            .map(str::to_owned)
            .collect()
    }

    /// Whether `text` names `method` in code, as `` `method `` or `.method`.
    fn mentions(text: &str, method: &str) -> bool {
        let is_name_char = |c: char| c.is_alphanumeric() || "_?!".contains(c);
        ["`", "."].iter().any(|prefix| {
            let needle = format!("{prefix}{method}");
            text.match_indices(&needle).any(|(start, _)| {
                let next = text[start + needle.len()..].chars().next();
                !next.is_some_and(is_name_char)
            })
        })
    }
}
