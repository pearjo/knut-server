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

## Domain model

- **Light** — one physical bulb, referenced by an adapter id (`hue:` or `zigbee:`).
- **Room** — named container of lights and groups.
- **Group** — several lights controlled as one entity. Exposed to HomeKit as one
  Lightbulb (on, brightness, color temperature). Individual lights stay hidden
  unless explicitly exposed.
- **Brightness mapping** — per light *within a group*: `map :spot3, brightness: ->(x) { x * 0.8 }`.
  `x` is the group's brightness 0–100 (what HomeKit shows). Result is clamped to
  1–100. Off stays off. Applies to every command sent to the group.
- **Scene** — named set of light states, `auto: true|false`. Exposed to HomeKit as
  a momentary switch (turns itself off after ~1 s; tappable and Siri-able).
- **Trigger** — `on "<source>", <event>, ... do ... end`. Sources: `"Sun"`, `"Clock"`,
  later device events (e.g. motion).

## Auto mode

- Every HomeKit-controlled entity (group, or individually exposed light) is either
  **in auto** or **overridden**.
- **Hard override**: the user changes brightness or color temperature (via HomeKit,
  or via a scene with `auto: false`). Only that entity leaves auto. Switching on
  alone is not an override — it comes on at the current sun values.
- **Back into auto**: switching the entity off and on again, or turning the Auto
  switch on.
- **Auto switch** (one HomeKit Switch):
  - shows **on** only if every entity is in auto, otherwise **off**,
  - turning it **on** puts every entity back into auto,
  - turning it **off** takes every entity out of auto (lights stay where they are).
- **Scenes with `auto: true`** set a starting point; from the next sun event the sun
  handler takes over and blends from there with a transition. A dim `auto: true`
  scene therefore drifts toward sun values within a minute.
- Apple's own Adaptive Lighting must stay disabled on Knut's lights.
- Native Home app scenes arrive as ordinary characteristic writes and therefore
  count as hard overrides.

## Sun

- Computed locally from `location lat:, lon:`.
- `on "Sun", :sun do |sun, now|` fires once per minute.
- `sun.elevation` (degrees), `sun.sunrise`, `sun.sunset` (today's times as `Time`).

## DSL example

```ruby
location lat: 52.99, lon: 10.58

hue    "bridge", host: "192.168.1.20"
zigbee "z2m",    mqtt: "mqtt://localhost:1883"

room "Küche" do
  light "spot1", zigbee: "0x…"
  light "spot2", zigbee: "0x…"
  light "spot3", zigbee: "0x…"
  light "spot4", zigbee: "0x…"

  group "Spots", lights: [:spot1, :spot2, :spot3, :spot4] do
    map :spot3, brightness: ->(x) { x * 0.8 }
  end
end

scene "Filmabend", auto: false do
  set "Wohnzimmer/Licht", brightness: 20, kelvin: 2200
end

scene "Morgen", auto: true do
  turn_on "Küche/Spots", "Flur/Licht"
end

on "Clock", at: "06:30" do
  activate "Morgen"
end

on "Sun", :sun do |sun, now|
  adapt all_groups, kelvin: sun.elevation > 10 ? 4000 : 2400, transition: 60
end
```

## CLI

- `knut check <config.rb>` — parse and validate, print the model or errors with line numbers.
- `knut discover` — list adapter devices not referenced in the config and print
  ready-to-paste DSL snippets.
- `knut set <Room/Entity> <value>` — control from the command line.
- `knut run` — the daemon.

## Persistence

HomeKit pairing keys and per-entity auto/override state live under `/var/lib/knut`.

## Build order

Each step runs on its own.

1. Skeleton: Cargo crate, mruby loads the DSL, model types, `knut check`.
2. zigbee2mqtt adapter + `knut discover`.
3. Hue adapter.
4. Groups, brightness mapping, `knut set`.
5. HomeKit bridge exposing groups.
6. Sun computation + `on "Sun", :sun`.
7. Scenes as momentary switches, auto mode and the Auto switch.
8. Clock triggers and device-event automations.
9. NixOS module (mDNS must cross VLANs for HomeKit).

## Code style

- Readable top to bottom; descriptive names.
- Comments only where context isn't obvious (e.g. magic numbers), never history.
- Doc comments on public API only.
- License: GPL-3.0 (kept from v1). Mascot: Knut the penguin.
