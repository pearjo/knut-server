//! Which sun and clock triggers are due on a tick.

use chrono::{DateTime, Local, NaiveDate, TimeZone};

use super::Engine;
use crate::model::{SunEvent, TriggerKind};
use crate::script::BlockArgs;
use crate::sun;

impl Engine {
    pub(super) async fn on_tick(
        &mut self,
        previous: DateTime<Local>,
        now: DateTime<Local>,
        first: bool,
    ) {
        let mut due: Vec<(usize, BlockArgs, String)> = Vec::new();
        let dates = [previous.date_naive(), now.date_naive()];

        let sun = self.model.location.as_ref().map(|location| {
            let position = sun::position(location.lat, location.lon, now.to_utc());
            let times = sun::times(location.lat, location.lon, now.date_naive());
            (location.lat, location.lon, position, times)
        });
        self.sun = sun.as_ref().map(|(_, _, position, _)| *position);

        for trigger in &self.model.triggers {
            let fire = |args: BlockArgs| (trigger.proc_id, args, trigger.line.clone());
            let sun_args =
                |position: &sun::SunPosition, times: &Option<sun::SunTimes>| BlockArgs::Sun {
                    position: *position,
                    times: *times,
                    now,
                };
            match &trigger.kind {
                TriggerKind::SunTick => {
                    if let Some((_, _, position, times)) = &sun {
                        due.push(fire(sun_args(position, times)));
                    }
                }
                TriggerKind::SunEvent {
                    event,
                    offset_minutes,
                } if !first => {
                    let Some((lat, lon, position, times)) = &sun else {
                        continue;
                    };
                    let happened = dates.iter().any(|date| {
                        sun::times(*lat, *lon, *date).is_some_and(|day| {
                            let at = match event {
                                SunEvent::Sunrise => day.sunrise,
                                SunEvent::Sunset => day.sunset,
                            } + chrono::Duration::minutes(*offset_minutes);
                            previous.to_utc() < at && at <= now.to_utc()
                        })
                    });
                    if happened {
                        due.push(fire(sun_args(position, times)));
                    }
                }
                TriggerKind::Clock { times } if !first => {
                    let happened = times.iter().any(|time| {
                        dates.iter().any(|date| {
                            local_time(*date, *time).is_some_and(|at| previous < at && at <= now)
                        })
                    });
                    if happened {
                        due.push(fire(BlockArgs::Clock { now }));
                    }
                }
                _ => {}
            }
        }
        for (proc_id, args, line) in due {
            self.run_trigger(proc_id, &args, &line).await;
        }
    }
}

fn local_time(date: NaiveDate, time: chrono::NaiveTime) -> Option<DateTime<Local>> {
    Local.from_local_datetime(&date.and_time(time)).earliest()
}
