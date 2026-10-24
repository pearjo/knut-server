# The Knut DSL. Loaded into mruby before the user's config.
#
# Loading the config only records a model; Rust reads it back as JSON via
# Knut.model_json. Scene and trigger blocks are stored as procs and run
# later by the daemon through Knut.run, which collects the actions they
# request (set, turn_on, activate, ...) and hands them back as JSON.

module Knut
  class ConfigError < StandardError; end

  PRELUDE_FILE = "(knut)"
  ADAPTER_KINDS = [:hue, :zigbee]

  @model = { location: nil, homekit: nil, schedule: nil, adapters: [], rooms: [], scenes: [], triggers: [] }
  @procs = []
  @actions = nil
  @state = {}

  class << self
    attr_reader :model, :actions, :state

    # "config.rb:12" of the innermost DSL call in the user's config. Frames
    # in mruby's own Ruby code (iterators like `map`) are skipped too.
    def location
      frame = caller.find { |line| !line.start_with?(PRELUDE_FILE) && !line.include?("/mrblib/") }
      return "?" unless frame
      suffix = frame.index(":in ")
      suffix ? frame[0, suffix] : frame
    end

    def fail(message)
      raise ConfigError, "#{location}: #{message}"
    end

    def register_proc(block)
      fail("a block is required") unless block
      @procs << block
      @procs.size - 1
    end

    def name!(value, what)
      fail("#{what} name must be a String, got #{value.inspect}") unless value.is_a?(String) && !value.empty?
      fail("#{what} name must not contain '/': #{value.inspect}") if value.include?("/")
      value
    end

    def number!(value, what)
      fail("#{what} must be a number, got #{value.inspect}") unless value.is_a?(Numeric)
      value.to_f
    end

    # "06:30" → 390, minutes after midnight.
    def clock_minutes!(value, what)
      digits = value.is_a?(String) && value.size == 5 && value[2] == ":" &&
        (value[0, 2] + value[3, 2]).each_char.all? { |c| c >= "0" && c <= "9" }
      hours, minutes = value[0, 2].to_i, value[3, 2].to_i if digits
      fail("#{what} must be an \"HH:MM\" time, got #{value.inspect}") unless digits && hours < 24 && minutes < 60
      hours * 60 + minutes
    end

    # [time, value] with time as minutes after midnight or an Anchor.
    def keyframe!(keyframe)
      fail("keyframes must be [time, value] pairs, got #{keyframe.inspect}") unless keyframe.is_a?(Array) && keyframe.size == 2
      time, value = keyframe
      time = case time
             when String then clock_minutes!(time, "keyframe time")
             when Symbol then Anchor.new(time, 0)
             when Anchor then time
             else fail("keyframe time must be \"HH:MM\", :wake, :bed or e.g. :bed - 1.h, got #{time.inspect}")
             end
      [time, number!(value, "keyframe value")]
    end

    # Splits `zigbee: "0x..", adapter: "z2m"` style options into the
    # adapter reference of a device.
    def device_ref(options, what)
      kinds = ADAPTER_KINDS.select { |kind| options.key?(kind) }
      fail("#{what} needs exactly one of #{ADAPTER_KINDS.map { |k| "#{k}:" }.join(", ")}") unless kinds.size == 1
      kind = kinds.first
      device = options[kind]
      fail("#{what} #{kind}: must be a String id") unless device.is_a?(String) && !device.empty?
      adapter = options[:adapter]
      fail("adapter: must be a String") unless adapter.nil? || adapter.is_a?(String)
      { adapter_kind: kind.to_s, adapter: adapter, device: device }
    end

    def model_json
      to_json(@model)
    end

    # Calls proc `id` with `args` while collecting the actions it requests.
    # `state` is a snapshot of every entity, keyed by path.
    def run(id, args, state)
      @state = state
      @actions = []
      @procs[id].call(*args)
      to_json(@actions)
    ensure
      @actions = nil
    end

    # Evaluates a brightness mapping for every group brightness 1..100.
    def mapping_table(id)
      mapping = @procs[id]
      table = (1..100).map do |x|
        value = mapping.call(x)
        raise ConfigError, "brightness mapping returned #{value.inspect} for #{x}, expected a number" unless value.is_a?(Numeric)
        value.to_f
      end
      to_json(table)
    end

    # Calls staging block `id` with `need` (0–1) and the time `now`; returns
    # the names of the lights it picked.
    def staging(id, need, now)
      lights = @procs[id].call(need, Time.at(now))
      unless lights.is_a?(Array)
        raise ConfigError, "staging block returned #{lights.inspect}, expected an Array of lights like [:spot1]"
      end
      to_json(lights.map(&:to_s))
    end

    def record(action)
      raise ConfigError, "#{action[:op]} can only be used inside a scene or trigger" unless @actions
      @actions << action
      nil
    end

    def paths(targets)
      paths = targets.flatten
      paths.each do |path|
        raise ConfigError, "expected a \"Room/Entity\" path, got #{path.inspect}" unless path.is_a?(String) && path.include?("/")
      end
      paths
    end

    def entity_paths(kinds)
      @model[:rooms].flat_map do |room|
        kinds.flat_map { |kind| room[kind].map { |entity| "#{room[:name]}/#{entity[:name]}" } }
      end
    end

    def to_json(value)
      case value
      when Hash
        "{" + value.map { |key, item| "#{to_json(key.to_s)}:#{to_json(item)}" }.join(",") + "}"
      when Array
        "[" + value.map { |item| to_json(item) }.join(",") + "]"
      when String
        json_string(value)
      when Symbol
        json_string(value.to_s)
      when Integer
        value.to_s
      when Float
        value.finite? ? value.to_s : "null"
      when true, false
        value.to_s
      when nil
        "null"
      when Time
        value.to_i.to_s
      else
        json_string(value.to_s)
      end
    end

    def json_string(text)
      escaped = text.each_char.map do |char|
        case char
        when '"' then '\\"'
        when "\\" then "\\\\"
        when "\n" then "\\n"
        when "\r" then "\\r"
        when "\t" then "\\t"
        else char.ord < 0x20 ? format("\\u%04x", char.ord) : char
        end
      end
      "\"#{escaped.join}\""
    end
  end

  class RoomBuilder
    # Window orientation as azimuth, degrees clockwise from north.
    DIRECTIONS = { north: 0, northeast: 45, east: 90, southeast: 135, south: 180, southwest: 225, west: 270, northwest: 315 }

    def initialize(room)
      @room = room
    end

    # daylight 0.8, facing: :east  or  facing: [:south, :west]
    def daylight(amount, facing:)
      Knut.fail("daylight is already set for room #{@room[:name].inspect}") if @room[:daylight]
      amount = Knut.number!(amount, "daylight")
      Knut.fail("daylight must be between 0 and 1, got #{amount}") unless amount >= 0 && amount <= 1
      directions = facing.is_a?(Array) ? facing : [facing]
      Knut.fail("facing: needs at least one direction") if directions.empty?
      unknown = directions.reject { |direction| DIRECTIONS.key?(direction) }
      unless unknown.empty?
        Knut.fail("unknown direction(s) for facing: #{unknown.map(&:inspect).join(", ")}, expected one of :#{DIRECTIONS.keys.join(", :")}")
      end
      @room[:daylight] = { amount: amount, facing: directions.map { |direction| DIRECTIONS[direction] } }
    end

    # light "spot1", zigbee: "0x…", expose: true
    def light(name, **options)
      Knut.name!(name, "light")
      unknown = options.keys - Knut::ADAPTER_KINDS - [:adapter, :expose]
      Knut.fail("unknown option(s) for light: #{unknown.join(", ")}") unless unknown.empty?
      entry = { name: name, expose: options[:expose], line: Knut.location }
      @room[:lights] << entry.merge(Knut.device_ref(options, "light #{name.inspect}"))
    end

    # sensor "Bewegung", zigbee: "0x…"
    def sensor(name, **options)
      Knut.name!(name, "sensor")
      unknown = options.keys - Knut::ADAPTER_KINDS - [:adapter]
      Knut.fail("unknown option(s) for sensor: #{unknown.join(", ")}") unless unknown.empty?
      entry = { name: name, line: Knut.location }
      @room[:sensors] << entry.merge(Knut.device_ref(options, "sensor #{name.inspect}"))
    end

    # group "Spots", lights: [:spot1, :spot2] do map :spot2, brightness: ->(x) { x * 0.8 } end
    def group(name, lights:, expose: true, &block)
      Knut.name!(name, "group")
      Knut.fail("lights: must be a non-empty Array") unless lights.is_a?(Array) && !lights.empty?
      group = { name: name, lights: lights.map(&:to_s), expose: expose, mappings: [], stages: nil, staging: nil, line: Knut.location }
      GroupBuilder.new(group).instance_eval(&block) if block
      @room[:groups] << group
    end
  end

  class GroupBuilder
    def initialize(group)
      @group = group
    end

    def map(light, brightness:)
      Knut.fail("brightness: must be a lambda, e.g. ->(x) { x * 0.8 }") unless brightness.respond_to?(:call)
      @group[:mappings] << { light: light.to_s, proc: Knut.register_proc(brightness), line: Knut.location }
    end

    # stages [:spot2], [:spot1, :spot3]
    def stages(*stages)
      Knut.fail("stages are already set for group #{@group[:name].inspect}") if @group[:stages]
      Knut.fail("a group takes either stages or staging, not both") if @group[:staging]
      Knut.fail("stages needs at least one stage, e.g. stages [:spot2], [:spot1, :spot3]") if stages.empty?
      unless stages.all? { |stage| stage.is_a?(Array) && !stage.empty? }
        Knut.fail("each stage must be a non-empty Array of lights, e.g. [:spot2]")
      end
      @group[:stages] = { lights: stages.map { |stage| stage.map(&:to_s) }, line: Knut.location }
    end

    # staging { |need, now| now.hour < 9 ? [:spot1, :spot2, :spot3] : [:spot2] }
    def staging(&block)
      Knut.fail("staging is already set for group #{@group[:name].inspect}") if @group[:staging]
      Knut.fail("a group takes either stages or staging, not both") if @group[:stages]
      @group[:staging] = { proc: Knut.register_proc(block), line: Knut.location }
    end
  end

  class ScheduleBuilder
    # In Time#wday order.
    DAYS = [:sunday, :monday, :tuesday, :wednesday, :thursday, :friday, :saturday]

    def initialize(days)
      @days = days
    end

    def weekdays(**times)
      set([1, 2, 3, 4, 5], "weekdays", times)
    end

    DAYS.each_with_index do |name, wday|
      define_method(name) { |**times| set([wday], name.to_s, times) }
    end

    private

    def set(wdays, what, times)
      unknown = times.keys - [:wake, :bed]
      Knut.fail("unknown option(s) for #{what}: #{unknown.join(", ")}") unless unknown.empty?
      Knut.fail("#{what} needs wake:, bed: or both") if times.empty?
      wake = times[:wake] && Knut.clock_minutes!(times[:wake], "#{what} wake:")
      bed = times[:bed] && Knut.clock_minutes!(times[:bed], "#{what} bed:")
      wdays.each do |wday|
        @days[wday][:wake] = wake if wake
        @days[wday][:bed] = bed if bed
      end
    end
  end

  ANCHORS = [:wake, :bed]

  # A keyframe time relative to the day's wake or bed time: :bed - 1.h
  class Anchor
    def initialize(name, offset)
      Knut.fail("only :wake and :bed can be used as times, got #{name.inspect}") unless ANCHORS.include?(name)
      Knut.fail("offsets must be whole minutes like 30.m or 3.h, got #{offset.inspect}") unless offset.is_a?(Integer)
      @name = name
      @offset = offset
    end

    # Minutes after midnight of the schedule day `day`.
    def minutes(day)
      day[@name] + @offset
    end
  end

  # A periodic 24 h curve through [time, value] keyframes; neighbouring
  # keyframes are joined with cosine easing.
  class Curve
    def initialize(keyframes, schedule)
      @keyframes = keyframes
      @schedule = schedule
    end

    # On every weekday the keyframe times must go round the clock exactly
    # once, in the order given.
    def check!
      (@schedule ? (0..6).to_a : [0]).each do |wday|
        times = times_on(wday)
        next if gaps(times).none?(&:zero?) && gaps(times).inject(:+) == 1440
        where = @schedule ? " on #{ScheduleBuilder::DAYS[wday]}" : ""
        clock = times.map { |time| format("%02d:%02d", time % 1440 / 60, time % 60) }
        Knut.fail("curve keyframes#{where} are not in clock order: #{clock.join(", ")}")
      end
    end

    def at(now)
      wday = now.wday
      minutes = now.hour * 60 + now.min + now.sec / 60.0
      if @schedule
        previous = (wday - 1) % 7
        # Until the middle of the night the previous day's wake and bed
        # times apply, so a bedtime after midnight belongs to its evening.
        middle_of_night = (@schedule[previous][:bed] - 1440 + @schedule[wday][:wake]) / 2.0
        wday = previous if minutes < middle_of_night
      end
      times = times_on(wday)
      lengths = gaps(times)
      i = (0...times.size).find { |index| (minutes - times[index]) % 1440 < lengths[index] }
      from = @keyframes[i][1]
      to = @keyframes[(i + 1) % @keyframes.size][1]
      progress = ((minutes - times[i]) % 1440) / lengths[i]
      from + (to - from) * (1 - Math.cos(Math::PI * progress)) / 2
    end

    private

    def times_on(wday)
      day = @schedule && @schedule[wday]
      @keyframes.map { |time, _| time.is_a?(Anchor) ? time.minutes(day) : time }
    end

    # Minutes from each keyframe to the next, going round the clock.
    def gaps(times)
      times.each_with_index.map { |time, i| (times[(i + 1) % times.size] - time) % 1440 }
    end
  end

  class Sun
    attr_reader :elevation, :azimuth, :sunrise, :sunset

    def initialize(elevation, azimuth, sunrise, sunset)
      @elevation = elevation
      @azimuth = azimuth
      @sunrise = sunrise && Time.at(sunrise)
      @sunset = sunset && Time.at(sunset)
    end

    def up?
      @elevation > 0
    end

    def inspect
      "#<Sun elevation=#{@elevation.round(2)} azimuth=#{@azimuth.round(2)}>"
    end
  end

  class Event
    attr_reader :device, :type, :data

    def initialize(device, type, data)
      @device = device
      @type = type.to_sym
      @data = data
    end

    def [](key)
      @data[key.to_s]
    end

    def action
      self[:action]
    end

    def method_missing(name, *args)
      key = name.to_s
      return @data[key] if args.empty? && @data.key?(key)
      super
    end

    def respond_to_missing?(name, include_private = false)
      @data.key?(name.to_s) || super
    end

    def inspect
      "#<Event #{@device} #{@type} #{@data.inspect}>"
    end
  end

  # What scene and trigger blocks see of an entity's current state.
  class EntityState
    def initialize(fields)
      @fields = fields || {}
    end

    def on?
      @fields["on"] == true
    end

    def brightness
      @fields["brightness"]
    end

    def kelvin
      @fields["kelvin"]
    end

    def auto?
      @fields["auto"] == true
    end
  end
end

# Durations for curve keyframes, in minutes: 30.m, 3.h
class Integer
  def m
    self
  end

  def h
    self * 60
  end
end

# :wake + 30.m, :bed - 1.h
class Symbol
  def +(minutes)
    Knut::Anchor.new(self, minutes)
  end

  def -(minutes)
    Knut::Anchor.new(self, -minutes)
  end
end

# ---------------------------------------------------------------------------
# Configuration

def location(lat:, lon:)
  Knut.fail("location is already set") if Knut.model[:location]
  lat = Knut.number!(lat, "lat")
  lon = Knut.number!(lon, "lon")
  Knut.fail("lat must be between -90 and 90") unless (-90..90).include?(lat)
  Knut.fail("lon must be between -180 and 180") unless (-180..180).include?(lon)
  Knut.model[:location] = { lat: lat, lon: lon, line: Knut.location }
end

# homekit name: "Knut", pin: "123-45-678", port: 32000, host: "192.168.1.5"
def homekit(name: "Knut", pin: nil, port: 32000, host: nil)
  Knut.fail("homekit is already configured") if Knut.model[:homekit]
  digits = pin.to_s.delete("-")
  unless pin.nil? || (digits.size == 8 && digits.each_char.all? { |c| c >= "0" && c <= "9" })
    Knut.fail("pin must look like \"123-45-678\"")
  end
  Knut.fail("port must be an Integer") unless port.is_a?(Integer)
  Knut.model[:homekit] = { name: name.to_s, pin: pin && pin.to_s.delete("-"), port: port, host: host && host.to_s, line: Knut.location }
end

# hue "bridge", host: "192.168.1.20"
def hue(name, host:, key: nil)
  Knut.name!(name, "hue bridge")
  Knut.model[:adapters] << { kind: "hue", name: name, host: host.to_s, key: key, line: Knut.location }
end

# zigbee "z2m", mqtt: "mqtt://localhost:1883"
def zigbee(name, mqtt:, base_topic: "zigbee2mqtt")
  Knut.name!(name, "zigbee")
  Knut.model[:adapters] << { kind: "zigbee", name: name, mqtt: mqtt.to_s, base_topic: base_topic.to_s, line: Knut.location }
end

# schedule do
#   weekdays wake: "06:30", bed: "23:00"
#   friday   bed: "00:30"
# end
def schedule(&block)
  Knut.fail("schedule is already set") if Knut.model[:schedule]
  Knut.fail("schedule needs a block") unless block
  days = Array.new(7) { {} }
  Knut::ScheduleBuilder.new(days).instance_eval(&block)
  incomplete = days.index { |day| !(day[:wake] && day[:bed]) }
  Knut.fail("schedule: #{Knut::ScheduleBuilder::DAYS[incomplete]} needs wake: and bed:") if incomplete
  Knut.model[:schedule] = days.map do |day|
    # A bedtime before the wake time is after midnight and belongs to this day.
    bed = day[:bed] < day[:wake] ? day[:bed] + 1440 : day[:bed]
    { wake: day[:wake], bed: bed }
  end
end

# demand = curve([:wake, 0.0], [:wake + 30.m, 1.0], ["17:00", 1.0], [:bed - 1.h, 0.25])
# demand.at(now)
def curve(*keyframes)
  Knut.fail("curve needs at least two [time, value] keyframes") if keyframes.size < 2
  keyframes = keyframes.map { |keyframe| Knut.keyframe!(keyframe) }
  schedule = Knut.model[:schedule]
  if !schedule && keyframes.any? { |time, _| time.is_a?(Knut::Anchor) }
    Knut.fail("curves with :wake or :bed need a schedule before them")
  end
  curve = Knut::Curve.new(keyframes, schedule)
  curve.check!
  curve
end

def room(name, &block)
  Knut.name!(name, "room")
  Knut.fail("room #{name.inspect} needs a block") unless block
  room = { name: name, lights: [], sensors: [], groups: [], daylight: nil, line: Knut.location }
  Knut::RoomBuilder.new(room).instance_eval(&block)
  Knut.model[:rooms] << room
end

def scene(name, auto: false, &block)
  Knut.fail("scene name must be a String") unless name.is_a?(String) && !name.empty?
  Knut.fail("auto: must be true or false") unless auto == true || auto == false
  Knut.model[:scenes] << { name: name, auto: auto, proc: Knut.register_proc(block), line: Knut.location }
end

# on "Sun", :sun do |sun, now| ... end
# on "Sun", :sunset, offset: -30 do |sun, now| ... end
# on "Clock", at: "06:30" do |now| ... end
# on "Flur/Bewegung", :motion do |event, now| ... end
def on(source, *events, **options, &block)
  Knut.fail("trigger source must be a String") unless source.is_a?(String)
  options = options.map { |key, value| [key.to_s, value.is_a?(Symbol) ? value.to_s : value] }.to_h
  Knut.model[:triggers] << {
    source: source,
    events: events.map(&:to_s),
    options: options,
    proc: Knut.register_proc(block),
    line: Knut.location
  }
end

# ---------------------------------------------------------------------------
# Actions, usable inside scene and trigger blocks

def set(*targets, on: nil, brightness: nil, kelvin: nil, transition: nil)
  if on.nil? && brightness.nil? && kelvin.nil?
    raise Knut::ConfigError, "set needs at least one of on:, brightness:, kelvin:"
  end
  Knut.record(op: "set", targets: Knut.paths(targets), on: on, brightness: brightness, kelvin: kelvin, transition: transition)
end

def turn_on(*targets, transition: nil)
  Knut.record(op: "set", targets: Knut.paths(targets), on: true, transition: transition)
end

def turn_off(*targets, transition: nil)
  Knut.record(op: "set", targets: Knut.paths(targets), on: false, transition: transition)
end

# Moves entities that are in auto mode toward the given values; `need`
# (0–1) is reduced by the room's daylight. Entities that are switched off
# only remember the values for when they come on.
def adapt(*targets, need: nil, kelvin: nil, transition: nil, brightness: nil)
  raise Knut::ConfigError, "adapt takes need: (0–1) instead of brightness:" unless brightness.nil?
  raise Knut::ConfigError, "adapt needs need:, kelvin: or both" if need.nil? && kelvin.nil?
  need = Knut.number!(need, "need:") unless need.nil?
  Knut.record(op: "adapt", targets: Knut.paths(targets), need: need, kelvin: kelvin, transition: transition)
end

def activate(scene_name)
  Knut.record(op: "activate", scene: scene_name.to_s)
end

def log(message)
  Knut.record(op: "log", message: message.to_s)
end

def all_groups
  Knut.entity_paths([:groups])
end

def all_lights
  Knut.entity_paths([:lights])
end

def state(path)
  Knut::EntityState.new(Knut.state[path])
end

def on?(path)
  state(path).on?
end
