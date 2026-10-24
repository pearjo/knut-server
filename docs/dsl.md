# The Knut configuration language

A Knut config is a Ruby file that describes your home: where it is, which
lights and sensors it has, how they are grouped, and what should happen when.
This page lists everything the language offers. For *why* Knut behaves the
way it does (auto mode, staging, daylight), see [DESIGN.md](../DESIGN.md).

Check a config at any time with

```sh
knut check config.rb
```

It prints the home as Knut understands it, or every problem with its line
number.

## Contents

1. [A first config](#a-first-config)
2. [Location and HomeKit](#location-and-homekit)
3. [Adapters](#adapters)
4. [Rooms, lights and sensors](#rooms-lights-and-sensors)
5. [Groups](#groups)
6. [Schedule and curves](#schedule-and-curves)
7. [Scenes](#scenes)
8. [Triggers](#triggers)
9. [Actions](#actions)
10. [Queries](#queries)
11. [What blocks receive](#what-blocks-receive)

## A first config

```ruby
location lat: 52.99, lon: 10.58

zigbee "z2m", mqtt: "mqtt://127.0.0.1:1883"

room "Kitchen" do
  light "spot1", zigbee: "0x0000000000000001"
  light "spot2", zigbee: "0x0000000000000002"

  group "Spots", lights: [:spot1, :spot2]
end

scene "Movie Night" do
  set "Kitchen/Spots", brightness: 20, kelvin: 2200
end

on "Sun", :sunset, offset: -30 do
  turn_on "Kitchen/Spots"
end
```

Every light, group and sensor has a **path**, `"Room/Name"`, like
`"Kitchen/Spots"`. Actions and triggers use paths to name what they mean.

A full example is in [examples/config.rb](../examples/config.rb).

## Location and HomeKit

### `location`

```ruby
location lat: 52.99, lon: 10.58
```

Where the home is. Knut computes the sun's position from it locally.

| Option | Value |
|---|---|
| `lat:` | Latitude in degrees, −90 to 90. Required. |
| `lon:` | Longitude in degrees, −180 to 180. Required. |

Needed for `"Sun"` triggers and room daylight. Set it once.

### `homekit`

```ruby
homekit name: "Knut", port: 32000
```

How the bridge appears in Apple Home. Optional; without it the defaults
below apply.

| Option | Value |
|---|---|
| `name:` | Name of the bridge. Default `"Knut"`. |
| `pin:` | Setup code, `"123-45-678"`. Without it, Knut picks one once and keeps it; the log shows it. Trivial codes such as `"111-11-111"` are rejected. |
| `port:` | TCP port. Default `32000`. |
| `host:` | IP address to serve on. Default: the first non-loopback interface. |

## Adapters

An adapter connects Knut to one device system. You can have several of each
kind.

### `zigbee`

```ruby
zigbee "z2m", mqtt: "mqtt://127.0.0.1:1883"
```

A [zigbee2mqtt](https://www.zigbee2mqtt.io) instance.

| Argument | Value |
|---|---|
| name | Name of the adapter, used with `adapter:` on lights and sensors. |
| `mqtt:` | URL of the MQTT broker. Required. |
| `base_topic:` | zigbee2mqtt's base topic. Default `"zigbee2mqtt"`. |

### `hue`

```ruby
hue "bridge", host: "192.168.1.20"
```

A Philips Hue bridge, controlled through its local API.

| Argument | Value |
|---|---|
| name | Name of the adapter. |
| `host:` | Address of the bridge. Required. |
| `key:` | The bridge's application key. Usually left out: `knut discover` pairs with the bridge (press its link button) and stores the key. |

## Rooms, lights and sensors

### `room`

```ruby
room "Kitchen" do
  daylight 0.8, facing: :east
  light "spot1", zigbee: "0x0000000000000001"
end
```

A room holds lights, sensors and groups. Names must not contain `/` and must
be unique within their room. Rooms in Apple Home are assigned in the Home
app; Knut's rooms only build paths and carry the daylight profile.

### `daylight`

```ruby
daylight 0.4, facing: [:south, :west]
```

How much daylight the room gets. Knut subtracts it from the need of the
room's groups when it adapts them (see [`adapt`](#adapt)).

| Argument | Value |
|---|---|
| amount | 0 to 1: how much daylight the room gets overall (window area, shading). |
| `facing:` | Window orientation, one or a list of `:north`, `:northeast`, `:east`, `:southeast`, `:south`, `:southwest`, `:west`, `:northwest`. |

Rooms without `daylight` get none subtracted. Set it at most once per room.

### `light`

```ruby
light "ceiling", hue: "1a2b3c4d-…", expose: true
```

One bulb.

| Option | Value |
|---|---|
| name | Name within the room. |
| `zigbee:` | The device's IEEE address, `"0x…"`. |
| `hue:` | The Hue light resource id. |
| `adapter:` | Name of the adapter. Only needed when there are several adapters of that kind. |
| `expose:` | Whether HomeKit shows this light on its own. Default: shown unless it is in a group. |

A light takes exactly one of `zigbee:` or `hue:`. `knut discover` prints a
ready-to-paste `light` line for every device not in the config yet.

### `sensor`

```ruby
sensor "Motion", zigbee: "0x0000000000000011"
```

A motion sensor, button or other device that sends events to
[triggers](#device-triggers). Takes the same `zigbee:`, `hue:` and `adapter:`
options as `light`. Sensors are not shown in HomeKit.

## Groups

### `group`

```ruby
group "Spots", lights: [:spot1, :spot2, :spot3] do
  stages [:spot2], [:spot1, :spot3]
  map :spot3, brightness: ->(x) { x * 0.8 }
end
```

Several lights of the room, controlled as one. HomeKit shows a group as one
lightbulb with brightness and colour temperature.

| Option | Value |
|---|---|
| name | Name within the room. |
| `lights:` | Names of lights in the same room, e.g. `[:spot1, :spot2]`. At least one. |
| `expose:` | Whether HomeKit shows the group. Default `true`. |

The block is optional and takes `stages` or `staging`, and `map`.

A group's value (0–100, what HomeKit shows as brightness) is its **need**:
how much light the group should deliver. Knut spreads the need over the
lights that are on.

### `stages`

```ruby
stages [:spot2], [:spot1, :spot3]   # middle one first, then both outer ones
```

Switches on only as many lights as the need requires. Each argument is one
stage, a list of lights; every light of the group is in exactly one stage.
At low need only the first stage is on; as the need rises, the next stages
join and the lights cross-fade.

### `staging`

```ruby
staging { |need, now| now.hour < 9 ? [:spot1, :spot2, :spot3] : [:spot2] }
```

Picks the lights itself, for cases `stages` cannot express. The block gets
the need (0 to 1) and the current time, and returns the lights to switch on.
If it fails or returns no or unknown lights, Knut logs it and uses all
lights. A group has either `stages` or `staging`, not both.

### `map`

```ruby
map :spot3, brightness: ->(x) { x * 0.8 }
```

Adjusts one light's brightness, e.g. to keep an accent light dimmer than the
others. The lambda gets the brightness the light receives (0 to 100) and
returns the one it shows. The result is kept between 1 and 100; a mapping
never switches a light on or off.

Knut evaluates the lambda once per value while the config loads, so it must
depend on `x` only.

## Schedule and curves

### `schedule`

```ruby
schedule do
  weekdays wake: "06:30", bed: "23:00"
  friday   bed: "00:30"
  saturday wake: "08:30", bed: "00:30"
  sunday   wake: "08:30", bed: "23:00"
end
```

When you get up and go to bed, per weekday. Curves use these times as
`:wake` and `:bed`.

Inside the block, `weekdays` sets Monday to Friday, and `monday`,
`tuesday`, `wednesday`, `thursday`, `friday`, `saturday` and `sunday` set
one day. Each takes `wake:`, `bed:` or both as `"HH:MM"`;
later lines override earlier ones. Every day needs both times in the end.

A bedtime before the wake time is after midnight and belongs to the evening
before: `friday bed: "00:30"` means Saturday, half past midnight.

### `curve`

```ruby
demand = curve(
  [:bed + 1.h,   0.0],
  [:wake,        0.0],
  [:wake + 30.m, 1.0],
  ["17:00",      1.0],
  [:bed - 1.h,   0.25])

demand.at(now)   # => the value at that time
```

A value that changes over the day, repeating every 24 hours. Each argument
is a keyframe `[time, value]`; between keyframes the value eases smoothly
from one to the next. Two equal values in a row hold the value.

| Keyframe time | Meaning |
|---|---|
| `"17:00"` | A fixed time of day. |
| `:wake`, `:bed` | The day's wake or bed time from the `schedule`. |
| `:wake + 30.m`, `:bed - 3.h` | Offsets in whole minutes (`.m`) or hours (`.h`). |

Curves with `:wake` or `:bed` need the `schedule` above them. The keyframes
must go round the clock once, in the order given, on every weekday; if a
schedule makes two keyframes swap places on some day, `knut check` names
that day.

`curve.at(time)` returns the value at `time`, usually the `now` a trigger
block receives.

## Scenes

### `scene`

```ruby
scene "Movie Night", auto: false do
  set "Living Room/Light", brightness: 20, kelvin: 2200
  turn_off "Kitchen/Spots"
end
```

A named set of [actions](#actions). HomeKit shows each scene as a switch
that turns itself off after a second, so it can be tapped or asked for with
Siri. `activate` runs a scene from other blocks.

| Option | Value |
|---|---|
| name | Name of the scene, unique in the config. |
| `auto:` | `false` (default): lights and groups whose brightness or colour temperature the scene sets leave auto mode and keep those values. `true`: the scene sets a starting point and auto mode takes over again from there. |

Knut runs every scene once while loading the config to check its actions.

## Triggers

### `on`

```ruby
on source, events..., options do |arguments| … end
```

Runs the block when something happens. The block contains
[actions](#actions) and may use [queries](#queries). Actions from triggers
never change auto mode.

#### Sun triggers

```ruby
on "Sun", :sun do |sun, now|
  adapt all_groups, need: demand.at(now), kelvin: warmth.at(now), transition: 60
end

on "Sun", :sunset, offset: -30 do |sun, now|
  turn_on "Living Room/Light"
end
```

| Event | When |
|---|---|
| `:sun` | Every minute. |
| `:sunrise` | At sunrise, shifted by `offset:` minutes (negative is earlier). |
| `:sunset` | At sunset, shifted by `offset:` minutes. |

The block gets the [sun](#the-sun) and the current time. Needs `location`.

#### Clock triggers

```ruby
on "Clock", at: "06:30" do |now|
  activate "Morning"
end
```

Runs at the given time every day. `at:` takes one `"HH:MM"` or a list of
them. The block gets the current time.

#### Device triggers

```ruby
on "Hallway/Motion", :motion do |event, now|
  turn_on "Hallway/Light" unless on?("Hallway/Light")
end
```

The source is the path of a sensor or light. Give one or more events:

| Event | When |
|---|---|
| `:motion` | A motion sensor sees motion. |
| `:clear` | A motion sensor no longer sees motion. |
| `:action` | A button or switch reports an action, e.g. a press. |
| `:change` | The device reports anything. |

The block gets the [event](#device-events) and the current time.

## Actions

Actions can only be used inside scene and trigger blocks. A **target** is a
path like `"Kitchen/Spots"`, a light or a group; lists such as `all_groups`
work too.

### `set`

```ruby
set "Kitchen/Spots", brightness: 40, kelvin: 2700, transition: 2
```

| Option | Value |
|---|---|
| `on:` | `true` or `false`. |
| `brightness:` | 1 to 100. |
| `kelvin:` | Colour temperature in kelvin, e.g. `2200` (warm) to `4000` (cool). |
| `transition:` | Seconds to fade. |

Needs at least one of `on:`, `brightness:` and `kelvin:`. With
`brightness:` or `kelvin:` it also switches the target on, unless
`on: false`.

### `turn_on`, `turn_off`

```ruby
turn_on "Kitchen/Spots", "Hallway/Light"
turn_off "Hallway/Light", transition: 10
```

Switch targets on or off, optionally with `transition:` in seconds. A
target in auto mode comes on with the values `adapt` last asked for.

### `adapt`

```ruby
adapt all_groups, "Living Room/Light", need: demand.at(now), kelvin: warmth.at(now), transition: 60
```

Moves targets that are in auto mode towards a need and colour temperature.
Targets that are off only remember the values for when they come on;
targets the user has overridden are left alone.

| Option | Value |
|---|---|
| `need:` | 0 to 1: how much artificial light is wanted. Knut subtracts the room's [daylight](#daylight) and keeps at least a comfort minimum while on. |
| `kelvin:` | Colour temperature in kelvin. |
| `transition:` | Seconds to fade. |

Needs `need:`, `kelvin:` or both. `adapt` takes `need:` instead of
`brightness:`.

### `activate`

```ruby
activate "Morning"
```

Runs a scene by name.

### `log`

```ruby
log "motion in the hallway"
```

Writes a line to Knut's log.

## Queries

### `all_groups`, `all_lights`

```ruby
adapt all_groups, kelvin: 2700
```

The paths of every group, or every light, in the config.

### `state`

```ruby
state("Kitchen/Spots").brightness
```

What a light or group is doing right now:

| Method | Value |
|---|---|
| `on?` | Whether it is on. |
| `brightness` | 1 to 100. |
| `kelvin` | Colour temperature, or `nil` if unknown. |
| `auto?` | Whether it is in auto mode. |

### `on?`

```ruby
turn_on "Hallway/Light" unless on?("Hallway/Light")
```

Short for `state(path).on?`.

## What blocks receive

### Time

`now` is a Ruby `Time`, e.g. `now.hour`, `now.min`, `now.wday` (0 is
Sunday).

### The sun

| Method | Value |
|---|---|
| `elevation` | Degrees above the horizon; negative at night. |
| `azimuth` | Degrees clockwise from north. |
| `sunrise`, `sunset` | Today's times as `Time`, or `nil` during polar day or night. |
| `up?` | Whether the sun is above the horizon. |

### Device events

| Method | Value |
|---|---|
| `device` | Path of the device, e.g. `"Hallway/Motion"`. |
| `type` | The event, e.g. `:motion`. |
| `action` | The action a button reported, as the device names it. |
| `data` | Everything the device reported, as a Hash. |
| `event[:key]`, `event.key` | One value of `data`, e.g. `event[:occupancy]` or `event.battery`. |

### Durations

`30.m` is 30 minutes and `3.h` is 3 hours, for curve keyframes such as
`:bed - 3.h`.
