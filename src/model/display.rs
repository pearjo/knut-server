//! The model as `knut check` prints it.

use std::fmt;

use super::{AdapterConfig, Group, Model, Room, ScheduleDay, Staging, TriggerKind};

/// Weekday names in `Weekday::num_days_from_sunday` order, as the schedule
/// is indexed.
const WEEKDAY_NAMES: [&str; 7] = [
    "sunday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
];

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.location {
            Some(location) => writeln!(f, "location  {:.4}, {:.4}", location.lat, location.lon)?,
            None => writeln!(f, "location  (none, sun triggers disabled)")?,
        }
        writeln!(
            f,
            "homekit   {:?} on port {}",
            self.homekit.name, self.homekit.port
        )?;
        if let Some(schedule) = &self.schedule {
            write_schedule(f, schedule)?;
        }
        write_adapters(f, &self.adapters)?;
        for room in &self.rooms {
            write_room(f, self, room)?;
        }
        write_scenes(f, self)?;
        write_triggers(f, self)
    }
}

/// Monday first; a bedtime after midnight names the day it falls on.
fn write_schedule(f: &mut fmt::Formatter<'_>, schedule: &[ScheduleDay; 7]) -> fmt::Result {
    writeln!(f)?;
    writeln!(f, "schedule")?;
    let clock = |minutes: u32| format!("{:02}:{:02}", minutes / 60 % 24, minutes % 60);
    for weekday in [1, 2, 3, 4, 5, 6, 0] {
        let day = schedule[weekday];
        let next_day = if day.bed >= 1440 {
            format!(" ({})", WEEKDAY_NAMES[(weekday + 1) % 7])
        } else {
            String::new()
        };
        let name = WEEKDAY_NAMES[weekday];
        writeln!(
            f,
            "  {name:<10} wake {}  bed {}{next_day}",
            clock(day.wake),
            clock(day.bed)
        )?;
    }
    Ok(())
}

fn write_adapters(f: &mut fmt::Formatter<'_>, adapters: &[AdapterConfig]) -> fmt::Result {
    writeln!(f)?;
    writeln!(f, "adapters")?;
    for adapter in adapters {
        match adapter {
            AdapterConfig::Hue { name, host, .. } => writeln!(f, "  hue     {name:<12} {host}")?,
            AdapterConfig::Zigbee {
                name,
                mqtt,
                base_topic,
                ..
            } => writeln!(f, "  zigbee  {name:<12} {mqtt} ({base_topic})")?,
        }
    }
    Ok(())
}

/// The room with its daylight, then its groups, lights and sensors.
fn write_room(f: &mut fmt::Formatter<'_>, model: &Model, room: &Room) -> fmt::Result {
    writeln!(f)?;
    match &room.daylight {
        Some(daylight) => {
            let facing: Vec<&str> = daylight
                .facing
                .iter()
                .map(|azimuth| compass_name(*azimuth))
                .collect();
            writeln!(
                f,
                "room {}  daylight {} facing {}",
                room.name,
                daylight.amount,
                facing.join(", ")
            )?
        }
        None => writeln!(f, "room {}", room.name)?,
    }
    for path in &room.groups {
        write_group(f, &model.groups[path])?;
    }
    for path in &room.lights {
        let light = &model.lights[path];
        let exposed = if light.exposed { " (exposed)" } else { "" };
        writeln!(f, "  light  {:<12} {}{exposed}", light.name, light.device)?;
    }
    for path in &room.sensors {
        let sensor = &model.sensors[path];
        writeln!(f, "  sensor {:<12} {}", sensor.name, sensor.device)?;
    }
    Ok(())
}

/// The group's members, each with a sample of its brightness mapping, and
/// its staging.
fn write_group(f: &mut fmt::Formatter<'_>, group: &Group) -> fmt::Result {
    let hidden = if group.exposed { "" } else { " (hidden)" };
    writeln!(f, "  group  {}{hidden}", group.name)?;
    for member in &group.members {
        let mapping = match &member.mapping {
            Some(mapping) => format!(
                "  brightness 25/50/100 → {:.0}/{:.0}/{:.0}",
                mapping.apply(25.0),
                mapping.apply(50.0),
                mapping.apply(100.0)
            ),
            None => String::new(),
        };
        let name = member.light.rsplit('/').next().unwrap_or(&member.light);
        writeln!(f, "    - {name}{mapping}")?;
    }
    match &group.staging {
        Some(Staging::Stages(stages)) => {
            let name = |path: &String| path.rsplit('/').next().unwrap_or(path).to_string();
            let stages: Vec<String> = stages
                .iter()
                .map(|stage| stage.iter().map(name).collect::<Vec<_>>().join(", "))
                .collect();
            writeln!(f, "    stages {}", stages.join(" | "))
        }
        Some(Staging::Custom { line, .. }) => writeln!(f, "    staging block at {line}"),
        None => Ok(()),
    }
}

fn write_scenes(f: &mut fmt::Formatter<'_>, model: &Model) -> fmt::Result {
    if model.scenes.is_empty() {
        return Ok(());
    }
    writeln!(f)?;
    writeln!(f, "scenes")?;
    for scene in &model.scenes {
        let mode = if scene.auto { "auto" } else { "override" };
        writeln!(f, "  {:<20} {mode}", scene.name)?;
    }
    Ok(())
}

fn write_triggers(f: &mut fmt::Formatter<'_>, model: &Model) -> fmt::Result {
    if model.triggers.is_empty() {
        return Ok(());
    }
    writeln!(f)?;
    writeln!(f, "triggers")?;
    for trigger in &model.triggers {
        let description = trigger_description(&trigger.kind);
        writeln!(f, "  {description:<36} {}", trigger.line)?;
    }
    Ok(())
}

/// What a trigger listens for, in one line.
fn trigger_description(kind: &TriggerKind) -> String {
    match kind {
        TriggerKind::SunTick => "Sun, every minute".to_string(),
        TriggerKind::SunEvent {
            event,
            offset_minutes,
        } => format!("Sun, {event:?} {offset_minutes:+} min"),
        TriggerKind::Clock { times } => {
            let times: Vec<String> = times
                .iter()
                .map(|time| time.format("%H:%M").to_string())
                .collect();
            format!("Clock at {}", times.join(", "))
        }
        TriggerKind::Device { path, events } => format!("{path} {}", events.join(", ")),
    }
}

fn compass_name(azimuth: f64) -> &'static str {
    const NAMES: [&str; 8] = [
        "north",
        "northeast",
        "east",
        "southeast",
        "south",
        "southwest",
        "west",
        "northwest",
    ];
    NAMES[(azimuth / 45.0).round() as usize % 8]
}
