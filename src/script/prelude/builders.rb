# DSL: the methods inside `room`, `group` and `schedule` blocks. Their
# public methods are part of the configuration language (docs/dsl.md).

module Knut
  # Inside `room "Kitchen" do … end`.
  class RoomBuilder
    # Window orientation as azimuth, degrees clockwise from north.
    DIRECTIONS = {
      north: 0,
      northeast: 45,
      east: 90,
      southeast: 135,
      south: 180,
      southwest: 225,
      west: 270,
      northwest: 315
    }

    def initialize(room)
      @room = room
    end

    # How much daylight the room gets: `amount` from 0 to 1, and the windows'
    # orientation as one compass point or a list.
    #
    #   daylight 0.4, facing: [:south, :west]
    def daylight(amount, facing:)
      if @room[:daylight]
        Internal.fail("daylight is already set for room #{@room[:name].inspect}")
      end
      amount = Internal.number!(amount, "daylight")
      unless amount >= 0 && amount <= 1
        Internal.fail("daylight must be between 0 and 1, got #{amount}")
      end
      @room[:daylight] = { amount: amount, facing: facing_azimuths(facing) }
    end

    # One bulb, with exactly one of `zigbee:` or `hue:`, and `adapter:` when
    # several adapters of that kind exist.
    #
    #   light "spot1", zigbee: "0x0000000000000001", expose: true
    def light(name, **options)
      Internal.name!(name, "light")
      Internal.reject_unknown_options!(options, ADAPTER_KINDS + [:adapter, :expose], "light")
      entry = { name: name, expose: options[:expose], line: Internal.location }
      @room[:lights] << entry.merge(Internal.device_ref(options, "light #{name.inspect}"))
    end

    # A device that sends events to triggers; options as for `light`.
    #
    #   sensor "Motion", zigbee: "0x0000000000000011"
    def sensor(name, **options)
      Internal.name!(name, "sensor")
      Internal.reject_unknown_options!(options, ADAPTER_KINDS + [:adapter], "sensor")
      entry = { name: name, line: Internal.location }
      @room[:sensors] << entry.merge(Internal.device_ref(options, "sensor #{name.inspect}"))
    end

    # Lights of this room controlled as one. The block may set `stages` or
    # `staging`, and `map`.
    #
    #   group "Spots", lights: [:spot1, :spot2] do … end
    def group(name, lights:, expose: true, &block)
      Internal.name!(name, "group")
      unless lights.is_a?(Array) && !lights.empty?
        Internal.fail("lights: must be a non-empty Array")
      end
      group = {
        name: name,
        lights: lights.map(&:to_s),
        expose: expose,
        mappings: [],
        stages: nil,
        staging: nil,
        line: Internal.location
      }
      GroupBuilder.new(group).instance_eval(&block) if block
      @room[:groups] << group
    end

    private

    def facing_azimuths(facing)
      directions = facing.is_a?(Array) ? facing : [facing]
      Internal.fail("facing: needs at least one direction") if directions.empty?
      unknown = directions.reject { |direction| DIRECTIONS.key?(direction) }
      unless unknown.empty?
        Internal.fail("unknown direction(s) for facing: #{unknown.map(&:inspect).join(", ")}, " \
                      "expected one of :#{DIRECTIONS.keys.join(", :")}")
      end
      directions.map { |direction| DIRECTIONS[direction] }
    end
  end

  # Inside `group "Spots", lights: […] do … end`.
  class GroupBuilder
    def initialize(group)
      @group = group
    end

    # Adjusts one light's brightness: the lambda gets the brightness the
    # light receives (0 to 100) and returns the one it shows.
    #
    #   map :spot3, brightness: ->(x) { x * 0.8 }
    def map(light, brightness:)
      unless brightness.respond_to?(:call)
        Internal.fail("brightness: must be a lambda, e.g. ->(x) { x * 0.8 }")
      end
      proc_id = Internal.register_proc(brightness)
      @group[:mappings] << { light: light.to_s, proc: proc_id, line: Internal.location }
    end

    # Switches on only as many stages, lists of lights, as the need requires.
    #
    #   stages [:spot2], [:spot1, :spot3]
    def stages(*stages)
      if @group[:stages]
        Internal.fail("stages are already set for group #{@group[:name].inspect}")
      end
      Internal.fail("a group takes either stages or staging, not both") if @group[:staging]
      if stages.empty?
        Internal.fail("stages needs at least one stage, e.g. stages [:spot2], [:spot1, :spot3]")
      end
      unless stages.all? { |stage| stage.is_a?(Array) && !stage.empty? }
        Internal.fail("each stage must be a non-empty Array of lights, e.g. [:spot2]")
      end
      lights = stages.map { |stage| stage.map(&:to_s) }
      @group[:stages] = { lights: lights, line: Internal.location }
    end

    # Picks the lights itself: the block gets the need (0 to 1) and the time,
    # and returns the lights to switch on.
    #
    #   staging { |need, now| now.hour < 9 ? [:spot1, :spot2] : [:spot2] }
    def staging(&block)
      if @group[:staging]
        Internal.fail("staging is already set for group #{@group[:name].inspect}")
      end
      Internal.fail("a group takes either stages or staging, not both") if @group[:stages]
      @group[:staging] = { proc: Internal.register_proc(block), line: Internal.location }
    end
  end

  # Inside `schedule do … end`: `weekdays` sets Monday to Friday, `monday` …
  # `sunday` one day, each with `wake:`, `bed:` or both as "HH:MM".
  #
  #   weekdays wake: "06:30", bed: "23:00"
  #   friday   bed: "00:30"
  class ScheduleBuilder
    def initialize(days)
      @days = days
    end

    def weekdays(**times)
      set([1, 2, 3, 4, 5], "weekdays", times)
    end

    WEEKDAYS.each_with_index do |name, wday|
      define_method(name) { |**times| set([wday], name.to_s, times) }
    end

    private

    def set(wdays, what, times)
      Internal.reject_unknown_options!(times, [:wake, :bed], what)
      Internal.fail("#{what} needs wake:, bed: or both") if times.empty?
      wake = times[:wake] && Internal.clock_minutes!(times[:wake], "#{what} wake:")
      bed = times[:bed] && Internal.clock_minutes!(times[:bed], "#{what} bed:")
      wdays.each do |wday|
        @days[wday][:wake] = wake if wake
        @days[wday][:bed] = bed if bed
      end
    end
  end
end
