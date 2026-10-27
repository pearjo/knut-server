# Knut 2.0 — Design

Knut is a small, focused smart-home daemon: lights, rooms, groups, scenes and
scripted automations, exposed to Apple Home. No frontend. Everything is
configured in a Ruby DSL.

## Stack

| Concern | Choice | Notes |
|---|---|---|
| Language | Rust, single binary | |
| DSL | mruby, statically linked via `minutus` | Fallback: thin own bindings on mruby's C API if `minutus` doesn't build against current mruby |
| HomeKit | `hap` crate (HAP over IP) as a bridge | Pre-1.0 and quiet; be prepared to fork/patch |
| Zigbee | zigbee2mqtt over MQTT | Coordinator is a TI CC2652 (Z-Stack). Native backend possible later behind the same adapter boundary |
| Hue | Hue bridge local API (CLIP v2, event stream) | |
| Tradfri gateway | not supported | Tradfri bulbs run via Zigbee |

zigbee2mqtt and Mosquitto run as separate services on the server.

The HomeKit layer talks only to groups, scenes and auto mode, never to device
adapters, so a Matter front end could later run alongside it.

## Domain model

- **Light** — one physical bulb, referenced by an adapter id (`hue:` or `zigbee:`).
- **Room** — named container of lights and groups, with a daylight profile.
- **Group** — several lights controlled as one entity. Exposed to HomeKit as one
  Lightbulb (on, brightness, color temperature). Individual lights stay hidden
  unless explicitly exposed.
- **Scene** — named set of light states, `auto: true|false`. Exposed to HomeKit as
  a momentary switch (turns itself off after ~1 s; tappable and Siri-able).
- **Trigger** — `on "<source>", <event>, ... do ... end`. Sources: `"Sun"`, `"Clock"`,
  later device events (e.g. motion).

## From demand to light

A group's value (0–100, what HomeKit shows as brightness) is its **need**: how much
artificial light the group should deliver. Each tick, per group:

```
need ──► staging ──► per-light brightness ──► brightness mapping ──► device
         (which lights)  (need spread over       (per-light curve,
                          the lights that are on)  e.g. dim accent)
```

1. **Need**
   - In the DSL, need and room daylight are 0–1; the group value is need × 100.
   - In auto: `need = demand(now) − room daylight(sun)`, never below a comfort
     minimum of 5 (on the 0–100 scale) while the group is on.
   - Manual override: the value set in HomeKit *is* the need. Room daylight is not
     subtracted.
2. **Staging** decides which lights are on, based on need only. A high need uses
   more stages than a low one — also under manual override.
3. **Distribution**: with `n` lights in the group and `k` of them on, each light
   gets `need · n / k`, capped at 100. All stages on ⇒ each light gets `need`.
4. **Brightness mapping** is applied per light to the brightness it receives after
   staging. A spot mapped to be dimmer stays dimmer whenever staging turns it on.
   Result clamped to 1–100; a mapping never switches a light on or off.

### Staging

```ruby
group "Spots", lights: [:spot1, :spot2, :spot3] do
  stages [:spot2], [:spot1, :spot3]   # middle one first, then both outer ones
end
```

- Every light of the group is in exactly one stage.
- Knut turns on the fewest leading stages whose lights can carry the need.
- Hysteresis: add the next stage when the active lights would exceed 95 %,
  drop it again only once the smaller set would stay below 75 %, so lights don't
  flicker at the threshold. The state is kept in memory, not persisted; after a
  restart staging starts without it.
- A newly added stage fades in while the others dim; total output stays constant.
  Stage changes take at least 1 s, also for HomeKit changes without a transition.
- Custom logic when `stages` isn't enough (`need` is 0–1; no hysteresis):

  ```ruby
  staging { |need, now| now.hour < 9 ? [:spot1, :spot2, :spot3] : [:spot2] }
  ```

  A block that fails, or returns no lights or lights outside the group, is logged
  and all lights are used.
- A group has either `stages` or `staging`, not both.
- Groups without `stages` / `staging`: all lights on, each gets `need`.
- `knut set` stages too, without hysteresis.

### Brightness mapping

Per light within a group, applied after staging (see above). `x` is the brightness
the light receives (0–100):

```ruby
map :spot3, brightness: ->(x) { x * 0.8 }
```

## Schedule and curves

### Schedule per weekday

```ruby
schedule do
  weekdays wake: "06:30", bed: "23:00"
  friday   bed: "00:30"
  saturday wake: "08:30", bed: "00:30"
  sunday   wake: "08:30", bed: "23:00"
end
```

`:wake` and `:bed` in curves resolve to the current day's times. A bedtime before
the wake time is after midnight and belongs to the previous evening, so a Friday
night runs into Saturday without a jump. The previous day's times apply until the
middle of the night (halfway between its bedtime and today's wake time). Every
weekday needs both times; `knut check` prints them.

### Keyframe curves

`curve(*keyframes)` builds a periodic 24 h curve from `[time, value]` pairs. Times
are `"HH:MM"` or offsets from `:wake` / `:bed`. Neighbouring keyframes are joined
with cosine easing (no kinks); two equal values form a plateau. `curve.at(now)`
evaluates it. Offsets are whole minutes: `30.m`, `3.h`.

Curves with `:wake` / `:bed` need the `schedule` before them. On every weekday the
resolved keyframe times must go round the clock once, in the order given;
keyframes that cross over on some day (e.g. `:bed - 3.h` before `"17:00"` on an
early night) are a config error naming that day.

```ruby
demand = curve(
  [:bed + 1.h,   0.0],
  [:wake,        0.0],
  [:wake + 30.m, 1.0],
  ["17:00",      1.0],
  [:bed - 3.h,   0.6],
  [:bed - 1.h,   0.25])

warmth = curve(
  [:bed + 1.h,   2200],
  [:wake,        2200],
  [:wake + 45.m, 4000],   # quickly cool to start the day
  ["15:00",      3000],   # gradually warmer
  [:bed - 3.h,   2800],   # evening plateau
  [:bed - 1.h,   2200])   # very warm for the late evening
```

Demand and warmth are driven by the clock (anchored on wake/bed); daylight is driven
by the sun.

### Background (research)

- Brown et al. 2022 (PLOS Biology): daytime ≥ 250 melanopic EDI lux at eye level,
  evening (from ~3 h before bed) ≤ 10, night ≤ 1.
- Intensity matters more than color temperature for melatonin (Souman et al. 2018);
  warm *and* dim in the evening is the effective lever.
- The Kruithof curve has no empirical support (Fotios 2017); the only robust comfort
  finding is to avoid very low light levels. Warmth and need are therefore
  independent curves.

## Room daylight

```ruby
room "Kitchen" do
  daylight 0.8, facing: :east
end

room "Living Room" do
  daylight 0.4, facing: [:south, :west]
end
```

- `daylight` (0–1): how much daylight the room gets overall (window area, shading).
- `facing`: window orientation(s), one of the 8 compass points (`:north`,
  `:northeast`, … `:northwest`).
- Room daylight = `min(1, daylight × (sky part + direct part))`:
  - **Sky part:** independent of orientation; eases in (cosine) with sun elevation
    from −6° to 30°.
  - **Direct part:** per window, only while the sun is above the horizon and in front
    of it: `cos(elevation) · max(0, cos(azimuth − facing))`, averaged over the
    windows (`daylight` already covers the total window area). Strongest with a low
    sun straight in front of the window. It fades in over the first 5° of elevation
    instead of jumping in at sunrise.
- Rooms without `daylight` get none subtracted.
- Weather is not modelled. An illuminance sensor (e.g. IKEA VALLHORN) could replace
  the estimate later.

## Auto mode

- Every HomeKit-controlled entity (group, or individually exposed light) is either
  **in auto** or **overridden**.
- **Hard override**: the user changes brightness or color temperature (via HomeKit,
  or via a scene with `auto: false`). Only that entity leaves auto. Switching on
  alone is not an override — it comes on at the current auto values.
- **Back into auto**: switching the entity off and on again, or turning the Auto
  switch on.
- **Auto switch** (one HomeKit Switch):
  - shows **on** only if every entity is in auto, otherwise **off**,
  - turning it **on** puts every entity back into auto,
  - turning it **off** takes every entity out of auto (lights stay where they are).
- **Scenes with `auto: true`** set a starting point; from the next sun event the sun
  handler takes over and blends from there with a transition.
- Overridden groups still use staging and brightness mapping; only the need comes
  from the user instead of the curves.
- `adapt` only adjusts groups that are on and in auto; it never switches a group on.
  For an entity that is off, it remembers the values, and the entity comes on with
  them.
- A light in auto that is switched on outside Knut (wall switch, Hue app) gets the
  current auto values as soon as it reports on: its own, or else its group's,
  through the brightness mapping. Device reports still never change auto mode.
- Actions from triggers never change auto mode. Only HomeKit, scenes and the Auto
  switch do.
- `turn_on` in an `auto: false` scene acts like switching on in HomeKit: if the
  entity was off, it goes back into auto.
- Apple's own Adaptive Lighting must stay disabled on Knut's lights.
- Native Home app scenes arrive as ordinary characteristic writes and therefore
  count as hard overrides.

## Sun

- Computed locally from `location lat:, lon:`.
- `on "Sun", :sun do |sun, now|` fires once per minute.
- `sun.elevation`, `sun.azimuth` (degrees, azimuth clockwise from north),
  `sun.sunrise`, `sun.sunset` (today's times as `Time`).

```ruby
on "Sun", :sun do |sun, now|
  adapt all_groups, need: demand.at(now), kelvin: warmth.at(now), transition: 60
end
```

`adapt` subtracts each group's room daylight from `need` itself. It takes groups
and individually exposed lights (a light counts as a group of one in its room),
and `need:`, `kelvin:` or both; `brightness:` is rejected in favour of `need:`.

## Other DSL

Locations, adapters, scenes, triggers, actions and queries are described in
the DSL reference, [docs/dsl.md](docs/dsl.md).

## HomeKit

- Rooms cannot be set by a bridge; they are assigned once in the Home app.
- Every exposed entity (group, scene, Auto switch) gets a **stable accessory ID**
  derived from its config name and persisted under `/var/lib/knut`, so reordering
  or extending the config never scrambles room assignments.
- The configuration number (`c#`) is bumped whenever the exposed accessories change.
- Pairings (iPhone and home hub) are persisted under `/var/lib/knut`.

## CLI

- `knut check <config.rb>` — parse and validate, print the model or errors with line numbers.
- `knut discover` — list adapter devices not referenced in the config and print
  ready-to-paste DSL snippets.
- `knut set <Room/Entity> <value>` — control from the command line.
- `knut run` — the daemon.

## Persistence

HomeKit pairing keys, accessory IDs and per-entity auto/override state live under
`/var/lib/knut`.

## Build order

Each step runs on its own.

1. Skeleton: Cargo crate, mruby loads the DSL, model types, `knut check`.
2. zigbee2mqtt adapter + `knut discover`.
3. Hue adapter.
4. Groups, brightness mapping, `knut set`.
5. HomeKit bridge exposing groups.
6. Sun computation (elevation, azimuth, sunrise, sunset) + `on "Sun", :sun`.
7. Scenes as momentary switches, auto mode and the Auto switch.
8. Clock triggers and device-event automations.
9. NixOS module (mDNS must cross VLANs for HomeKit).
10. Schedule per weekday, keyframe curves, room daylight, need-based `adapt`.
11. Staging (`stages`, custom `staging` block, hysteresis, cross-fade).

## Code style

See [CONTRIBUTE.md](CONTRIBUTE.md), "Conventions".

License: GPL-3.0 (kept from v1). Mascot: Knut the penguin.
