# DSL: the objects a config works with. Curves come from `curve`; scene and
# trigger blocks receive the sun, device events and entity states. Their
# public methods are part of the configuration language (docs/dsl.md).

module Knut
  ANCHORS = [:wake, :bed]

  # A keyframe time relative to the day's wake or bed time, e.g. :bed - 1.h.
  class Anchor
    def initialize(name, offset)
      unless ANCHORS.include?(name)
        Internal.fail("only :wake and :bed can be used as times, got #{name.inspect}")
      end
      unless offset.is_a?(Integer)
        Internal.fail("offsets must be whole minutes like 30.m or 3.h, got #{offset.inspect}")
      end
      @name = name
      @offset = offset
    end

    # Minutes after midnight of the schedule day `day`.
    def minutes(day)
      day[@name] + @offset
    end
  end

  # A value that changes over the day, repeating every 24 hours, through
  # [time, value] keyframes. Neighbouring keyframes are joined with cosine
  # easing.
  class Curve
    def initialize(keyframes, schedule)
      @keyframes = keyframes
      @schedule = schedule
      check!
    end

    # The value at the Time `now`.
    def at(now)
      minutes = now.hour * 60 + now.min + now.sec / 60.0
      times = times_on(schedule_day(now, minutes))
      lengths = gaps(times)
      index = (0...times.size).find { |i| (minutes - times[i]) % MINUTES_PER_DAY < lengths[i] }
      from = @keyframes[index][1]
      to = @keyframes[(index + 1) % @keyframes.size][1]
      progress = ((minutes - times[index]) % MINUTES_PER_DAY) / lengths[index]
      ease(from, to, progress)
    end

    private

    # On every weekday the keyframe times must go round the clock exactly
    # once, in the order given.
    def check!
      days = @schedule ? (0..6).to_a : [0]
      days.each do |wday|
        times = times_on(wday)
        next if round_the_clock?(times)
        where = @schedule ? " on #{WEEKDAYS[wday]}" : ""
        clock = times.map { |time| clock_text(time) }
        Internal.fail("curve keyframes#{where} are not in clock order: #{clock.join(", ")}")
      end
    end

    # The weekday whose schedule applies at `now`. Until the middle of the
    # night the previous day's wake and bed times apply, so a bedtime after
    # midnight belongs to its evening.
    def schedule_day(now, minutes)
      wday = now.wday
      return wday unless @schedule
      previous = (wday - 1) % 7
      previous_bed = @schedule[previous][:bed] - MINUTES_PER_DAY
      middle_of_night = (previous_bed + @schedule[wday][:wake]) / 2.0
      minutes < middle_of_night ? previous : wday
    end

    # The keyframe times in minutes after midnight of weekday `wday`.
    def times_on(wday)
      day = @schedule && @schedule[wday]
      @keyframes.map { |time, _| time.is_a?(Anchor) ? time.minutes(day) : time }
    end

    # Minutes from each keyframe to the next, going round the clock.
    def gaps(times)
      times.each_with_index.map do |time, i|
        (times[(i + 1) % times.size] - time) % MINUTES_PER_DAY
      end
    end

    def round_the_clock?(times)
      lengths = gaps(times)
      lengths.none?(&:zero?) && lengths.inject(:+) == MINUTES_PER_DAY
    end

    # Cosine easing from `from` to `to`, with `progress` from 0 to 1.
    def ease(from, to, progress)
      from + (to - from) * (1 - Math.cos(Math::PI * progress)) / 2
    end

    def clock_text(minutes)
      format("%02d:%02d", minutes % MINUTES_PER_DAY / 60, minutes % 60)
    end
  end

  # The sun, as `on "Sun"` blocks receive it.
  class Sun
    # Degrees above the horizon, and degrees clockwise from north.
    attr_reader :elevation, :azimuth

    # Today's sunrise and sunset as Time, or nil during polar day or night.
    attr_reader :sunrise, :sunset

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

  # What a sensor or light reported, as device triggers receive it.
  # `event[:occupancy]` and `event.occupancy` read one value of the report.
  class Event
    # The device's path, the event (e.g. :motion) and the whole report.
    attr_reader :device, :type, :data

    def initialize(device, type, data)
      @device = device
      @type = type.to_sym
      @data = data
    end

    def [](key)
      @data[key.to_s]
    end

    # The action a button reported, as the device names it.
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

    # 1 to 100.
    def brightness
      @fields["brightness"]
    end

    # Colour temperature in kelvin, or nil if unknown.
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

# Keyframe times relative to the schedule: :wake + 30.m, :bed - 1.h
class Symbol
  def +(minutes)
    Knut::Anchor.new(self, minutes)
  end

  def -(minutes)
    Knut::Anchor.new(self, -minutes)
  end
end
