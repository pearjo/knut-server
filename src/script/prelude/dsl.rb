# DSL: the top-level methods of a config. This is the configuration
# language; docs/dsl.md describes it for people writing configs.

module Knut
  # Included into every object, so that the methods also work inside room,
  # group, scene and trigger blocks.
  module DSL
    # -------------------------------------------------------------------------
    # Configuration

    # Where the home is, for the sun: latitude −90 to 90, longitude −180 to 180.
    #
    #   location lat: 52.99, lon: 10.58
    def location(lat:, lon:)
      Internal.fail("location is already set") if Internal.model[:location]
      lat = Internal.number!(lat, "lat")
      lon = Internal.number!(lon, "lon")
      Internal.fail("lat must be between -90 and 90") unless (-90..90).include?(lat)
      Internal.fail("lon must be between -180 and 180") unless (-180..180).include?(lon)
      Internal.model[:location] = { lat: lat, lon: lon, line: Internal.location }
    end

    # How the bridge appears in Apple Home. `pin:` is the setup code; without
    # it Knut picks one. `host:` is the address to serve on.
    #
    #   homekit name: "Knut", pin: "123-45-678", port: 32000
    def homekit(name: "Knut", pin: nil, port: 32000, host: nil)
      Internal.fail("homekit is already configured") if Internal.model[:homekit]
      pin_digits = pin && pin.to_s.delete("-")
      unless pin.nil? || (pin_digits.size == 8 && Internal.digits?(pin_digits))
        Internal.fail("pin must look like \"123-45-678\"")
      end
      Internal.fail("port must be an Integer") unless port.is_a?(Integer)
      Internal.model[:homekit] = {
        name: name.to_s,
        pin: pin_digits,
        port: port,
        host: host && host.to_s,
        line: Internal.location
      }
    end

    # A Philips Hue bridge. `key:` is its application key; `knut discover`
    # pairs and stores one when it is left out.
    #
    #   hue "bridge", host: "192.168.1.20"
    def hue(name, host:, key: nil)
      Internal.name!(name, "hue bridge")
      Internal.model[:adapters] << {
        kind: "hue",
        name: name,
        host: host.to_s,
        key: key,
        line: Internal.location
      }
    end

    # A zigbee2mqtt instance, reached through its MQTT broker.
    #
    #   zigbee "z2m", mqtt: "mqtt://127.0.0.1:1883"
    def zigbee(name, mqtt:, base_topic: "zigbee2mqtt")
      Internal.name!(name, "zigbee")
      Internal.model[:adapters] << {
        kind: "zigbee",
        name: name,
        mqtt: mqtt.to_s,
        base_topic: base_topic.to_s,
        line: Internal.location
      }
    end

    # Wake and bed time per weekday, for `:wake` and `:bed` in curves. Every
    # day needs both; a bedtime before the wake time is after midnight.
    #
    #   schedule do
    #     weekdays wake: "06:30", bed: "23:00"
    #     friday   bed: "00:30"
    #   end
    def schedule(&block)
      Internal.fail("schedule is already set") if Internal.model[:schedule]
      Internal.fail("schedule needs a block") unless block
      days = Array.new(7) { {} }
      ScheduleBuilder.new(days).instance_eval(&block)
      incomplete = days.index { |day| !(day[:wake] && day[:bed]) }
      if incomplete
        Internal.fail("schedule: #{WEEKDAYS[incomplete]} needs wake: and bed:")
      end
      Internal.model[:schedule] = days.map do |day|
        # A bedtime before the wake time is after midnight and belongs to this day.
        after_midnight = day[:bed] < day[:wake]
        bed = after_midnight ? day[:bed] + MINUTES_PER_DAY : day[:bed]
        { wake: day[:wake], bed: bed }
      end
    end

    # A value over the day through [time, value] keyframes; times are
    # "HH:MM", :wake, :bed or offsets like :bed - 1.h. `curve.at(now)`
    # evaluates it.
    #
    #   demand = curve([:wake, 0.0], [:wake + 30.m, 1.0], ["17:00", 1.0], [:bed - 1.h, 0.25])
    def curve(*keyframes)
      Internal.fail("curve needs at least two [time, value] keyframes") if keyframes.size < 2
      keyframes = keyframes.map { |keyframe| Internal.keyframe!(keyframe) }
      schedule = Internal.model[:schedule]
      anchored = keyframes.any? { |time, _| time.is_a?(Anchor) }
      if anchored && !schedule
        Internal.fail("curves with :wake or :bed need a schedule before them")
      end
      Curve.new(keyframes, schedule)
    end

    # A room with its lights, sensors, groups and daylight profile.
    #
    #   room "Kitchen" do
    #     light "spot1", zigbee: "0x0000000000000001"
    #   end
    def room(name, &block)
      Internal.name!(name, "room")
      Internal.fail("room #{name.inspect} needs a block") unless block
      room = {
        name: name,
        lights: [],
        sensors: [],
        groups: [],
        daylight: nil,
        line: Internal.location
      }
      RoomBuilder.new(room).instance_eval(&block)
      Internal.model[:rooms] << room
    end

    # A named set of actions, shown in HomeKit as a switch. With
    # `auto: false` the entities it sets leave auto mode; with `auto: true`
    # auto mode takes over from the scene's values.
    #
    #   scene "Movie Night", auto: false do
    #     set "Living Room/Light", brightness: 20, kelvin: 2200
    #   end
    def scene(name, auto: false, &block)
      Internal.fail("scene name must be a String") unless Internal.non_empty_string?(name)
      Internal.fail("auto: must be true or false") unless auto == true || auto == false
      Internal.model[:scenes] << {
        name: name,
        auto: auto,
        proc: Internal.register_proc(block),
        line: Internal.location
      }
    end

    # Runs the block when something happens: "Sun" with :sun (every
    # minute), :sunrise or :sunset (with `offset:` minutes); "Clock" with
    # `at:`; or a device path with :motion, :clear, :action or :change.
    #
    #   on "Sun", :sunset, offset: -30 do |sun, now| … end
    #   on "Clock", at: "06:30" do |now| … end
    #   on "Hallway/Motion", :motion do |event, now| … end
    def on(source, *events, **options, &block)
      Internal.fail("trigger source must be a String") unless source.is_a?(String)
      options = options.map { |key, value| [key.to_s, value.is_a?(Symbol) ? value.to_s : value] }
      Internal.model[:triggers] << {
        source: source,
        events: events.map(&:to_s),
        options: options.to_h,
        proc: Internal.register_proc(block),
        line: Internal.location
      }
    end

    # -------------------------------------------------------------------------
    # Actions, inside scene and trigger blocks. Targets are "Room/Entity"
    # paths or lists of them.

    # Sets lights or groups. `brightness:` is 1 to 100, `kelvin:` the colour
    # temperature, `transition:` seconds. With brightness or kelvin it also
    # switches on, unless `on: false`.
    #
    #   set "Kitchen/Spots", brightness: 40, kelvin: 2700, transition: 2
    def set(*targets, on: nil, brightness: nil, kelvin: nil, transition: nil)
      if on.nil? && brightness.nil? && kelvin.nil?
        raise ConfigError, "set needs at least one of on:, brightness:, kelvin:"
      end
      Internal.record(
        op: "set",
        targets: Internal.paths(targets),
        on: on,
        brightness: brightness,
        kelvin: kelvin,
        transition: transition
      )
    end

    #   turn_on "Kitchen/Spots", "Hallway/Light", transition: 2
    def turn_on(*targets, transition: nil)
      Internal.record(
        op: "set",
        targets: Internal.paths(targets),
        on: true,
        transition: transition
      )
    end

    #   turn_off "Hallway/Light", transition: 10
    def turn_off(*targets, transition: nil)
      Internal.record(
        op: "set",
        targets: Internal.paths(targets),
        on: false,
        transition: transition
      )
    end

    # Moves entities that are in auto mode toward a need (0 to 1, reduced by
    # the room's daylight) and colour temperature. Entities that are off
    # only remember the values for when they come on.
    #
    #   adapt all_groups, need: demand.at(now), kelvin: warmth.at(now), transition: 60
    def adapt(*targets, need: nil, kelvin: nil, transition: nil, brightness: nil)
      raise ConfigError, "adapt takes need: (0–1) instead of brightness:" unless brightness.nil?
      raise ConfigError, "adapt needs need:, kelvin: or both" if need.nil? && kelvin.nil?
      need = Internal.number!(need, "need:") unless need.nil?
      Internal.record(
        op: "adapt",
        targets: Internal.paths(targets),
        need: need,
        kelvin: kelvin,
        transition: transition
      )
    end

    # Runs a scene by name.
    #
    #   activate "Morning"
    def activate(scene_name)
      Internal.record(op: "activate", scene: scene_name.to_s)
    end

    # Writes a line to Knut's log.
    #
    #   log "motion in the hallway"
    def log(message)
      Internal.record(op: "log", message: message.to_s)
    end

    # -------------------------------------------------------------------------
    # Queries

    # The paths of every group in the config.
    def all_groups
      Internal.entity_paths([:groups])
    end

    # The paths of every light in the config.
    def all_lights
      Internal.entity_paths([:lights])
    end

    # What a light or group is doing right now: `on?`, `brightness`,
    # `kelvin`, `auto?`.
    #
    #   state("Kitchen/Spots").brightness
    def state(path)
      EntityState.new(Internal.state[path])
    end

    #   turn_on "Hallway/Light" unless on?("Hallway/Light")
    def on?(path)
      state(path).on?
    end
  end
end

class Object
  include Knut::DSL
end
