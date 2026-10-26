//! Which sun and clock triggers are due on a tick.

use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone};

use super::Engine;
use crate::model::{Location, Model, SunEvent, TriggerKind};
use crate::script::BlockArgs;
use crate::sun;

/// A trigger block to run, with the arguments it receives.
#[derive(Debug)]
pub(super) struct DueTrigger {
    pub proc_id: usize,
    pub args: BlockArgs,
    /// Where the trigger is in the config, for error messages.
    pub line: String,
}

impl Engine {
    /// Runs every sun and clock trigger due since the tick at `previous`.
    pub(super) async fn on_tick(
        &mut self,
        previous: DateTime<Local>,
        now: DateTime<Local>,
        first: bool,
    ) {
        self.sun = self
            .model
            .location
            .as_ref()
            .map(|location| sun::position(location.lat, location.lon, now.to_utc()));
        for due in due_triggers(&self.model, previous, now, first) {
            self.run_trigger(due.proc_id, &due.args, &due.line).await;
        }
    }
}

/// The sun and clock triggers due between the ticks at `previous` and `now`.
/// On the `first` tick only the every-minute sun trigger fires, because
/// `previous` is not a real tick yet.
pub(super) fn due_triggers(
    model: &Model,
    previous: DateTime<Local>,
    now: DateTime<Local>,
    first: bool,
) -> Vec<DueTrigger> {
    let sun_args = model.location.as_ref().map(|location| BlockArgs::Sun {
        position: sun::position(location.lat, location.lon, now.to_utc()),
        times: sun::times(location.lat, location.lon, now.date_naive()),
        now,
    });
    let mut due = Vec::new();
    for trigger in &model.triggers {
        let args = match &trigger.kind {
            TriggerKind::SunTick => sun_args.clone(),
            TriggerKind::SunEvent {
                event,
                offset_minutes,
            } if !first => {
                let happened = model.location.as_ref().is_some_and(|location| {
                    sun_event_between(location, *event, *offset_minutes, previous, now)
                });
                sun_args.clone().filter(|_| happened)
            }
            TriggerKind::Clock { times } if !first => {
                let happened = clock_time_between(times, previous, now);
                happened.then_some(BlockArgs::Clock { now })
            }
            _ => None,
        };
        if let Some(args) = args {
            due.push(DueTrigger {
                proc_id: trigger.proc_id,
                args,
                line: trigger.line.clone(),
            });
        }
    }
    due
}

/// Whether sunrise or sunset, shifted by `offset_minutes`, falls after
/// `previous` and at or before `now`.
fn sun_event_between(
    location: &Location,
    event: SunEvent,
    offset_minutes: i64,
    previous: DateTime<Local>,
    now: DateTime<Local>,
) -> bool {
    let dates = [previous.date_naive(), now.date_naive()];
    dates.iter().any(|date| {
        sun::times(location.lat, location.lon, *date).is_some_and(|day| {
            let at = match event {
                SunEvent::Sunrise => day.sunrise,
                SunEvent::Sunset => day.sunset,
            } + chrono::Duration::minutes(offset_minutes);
            previous.to_utc() < at && at <= now.to_utc()
        })
    })
}

/// Whether one of the daily `times` falls after `previous` and at or before
/// `now`.
fn clock_time_between(
    times: &[NaiveTime],
    previous: DateTime<Local>,
    now: DateTime<Local>,
) -> bool {
    let dates = [previous.date_naive(), now.date_naive()];
    times.iter().any(|time| {
        dates
            .iter()
            .any(|date| local_time(*date, *time).is_some_and(|at| previous < at && at <= now))
    })
}

fn local_time(date: NaiveDate, time: NaiveTime) -> Option<DateTime<Local>> {
    Local.from_local_datetime(&date.and_time(time)).earliest()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::Script;

    const TRIGGERS: &str = r#"
location lat: 52.99, lon: 10.58
on "Sun", :sun do |sun, now| end
on "Sun", :sunset, offset: -30 do |sun, now| end
on "Clock", at: ["06:30", "18:00"] do |now| end
"#;

    fn model() -> Model {
        let (_, model) = Script::load_str(TRIGGERS, "config.rb").unwrap();
        model
    }

    /// The config lines of the triggers due in the minute ending at `now`.
    fn due_lines(model: &Model, now: DateTime<Local>, first: bool) -> Vec<String> {
        let previous = now - chrono::Duration::minutes(1);
        due_triggers(model, previous, now, first)
            .into_iter()
            .map(|due| due.line)
            .collect()
    }

    fn at(hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 10, 22, hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn the_sun_trigger_fires_every_minute_and_alone_on_the_first_tick() {
        let model = model();
        assert_eq!(due_lines(&model, at(10, 0), false), ["config.rb:3"]);
        assert_eq!(
            due_lines(&model, at(6, 30), true),
            ["config.rb:3"],
            "the first tick has no real previous tick"
        );
    }

    #[test]
    fn clock_triggers_fire_in_the_minute_of_each_time() {
        let model = model();
        assert_eq!(
            due_lines(&model, at(6, 30), false),
            ["config.rb:3", "config.rb:5"]
        );
        assert_eq!(
            due_lines(&model, at(18, 0), false),
            ["config.rb:3", "config.rb:5"]
        );
        assert_eq!(due_lines(&model, at(6, 31), false), ["config.rb:3"]);
    }

    #[test]
    fn sun_events_fire_at_the_shifted_time() {
        let model = model();
        let location = model.location.as_ref().unwrap();
        let sunset = sun::times(location.lat, location.lon, at(12, 0).date_naive())
            .unwrap()
            .sunset
            .with_timezone(&Local);
        let thirty_before = sunset - chrono::Duration::minutes(30);
        let minute_after = thirty_before + chrono::Duration::seconds(59);

        assert_eq!(
            due_lines(&model, minute_after, false),
            ["config.rb:3", "config.rb:4"]
        );
        assert_eq!(due_lines(&model, sunset, false), ["config.rb:3"]);
    }
}
