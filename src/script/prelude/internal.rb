# Internals: what the DSL needs to record a config. Not part of the
# configuration language.
#
# Loading a config only records a model of the home. Scene and trigger
# blocks are stored as procs; when the daemon runs one, the actions it asks
# for (set, turn_on, activate, ...) are collected instead of carried out.

module Knut
  class ConfigError < StandardError; end

  # Every prelude file is loaded under a name starting with this, so that
  # errors can point past the prelude to the config line.
  PRELUDE_FILE = "(knut)"

  ADAPTER_KINDS = [:hue, :zigbee]

  # In Time#wday order.
  WEEKDAYS = [:sunday, :monday, :tuesday, :wednesday, :thursday, :friday, :saturday]

  MINUTES_PER_DAY = 24 * 60

  module Internal
    @model = {
      location: nil,
      homekit: nil,
      schedule: nil,
      adapters: [],
      rooms: [],
      scenes: [],
      triggers: []
    }
    @procs = []
    @actions = nil
    @state = {}

    class << self
      # The home as recorded so far.
      attr_reader :model

      # The snapshot of every entity's state, keyed by path, while a block runs.
      attr_reader :state

      # Stores a block for later and returns its id.
      def register_proc(block)
        fail("a block is required") unless block
        @procs << block
        @procs.size - 1
      end

      def registered_proc(id)
        @procs[id]
      end

      # Runs the given block with `state` as the entity snapshot and returns
      # the actions it recorded.
      def collecting_actions(state)
        @state = state
        @actions = []
        yield
        @actions
      ensure
        @actions = nil
      end

      def record(action)
        unless @actions
          raise ConfigError, "#{action[:op]} can only be used inside a scene or trigger"
        end
        @actions << action
        nil
      end

      # ------------------------------------------------------------------
      # Errors

      # "config.rb:12" of the innermost DSL call in the user's config. Frames
      # in mruby's own Ruby code (iterators like `map`) are skipped too.
      def location
        frame = caller.find { |line| config_frame?(line) }
        return "?" unless frame
        suffix = frame.index(":in ")
        suffix ? frame[0, suffix] : frame
      end

      def fail(message)
        raise ConfigError, "#{location}: #{message}"
      end

      # ------------------------------------------------------------------
      # Validation

      def name!(value, what)
        unless non_empty_string?(value)
          fail("#{what} name must be a String, got #{value.inspect}")
        end
        if value.include?("/")
          fail("#{what} name must not contain '/': #{value.inspect}")
        end
        value
      end

      def number!(value, what)
        fail("#{what} must be a number, got #{value.inspect}") unless value.is_a?(Numeric)
        value.to_f
      end

      # "06:30" → 390, minutes after midnight.
      def clock_minutes!(value, what)
        unless clock_time?(value)
          fail("#{what} must be an \"HH:MM\" time, got #{value.inspect}")
        end
        hours = value[0, 2].to_i
        minutes = value[3, 2].to_i
        hours * 60 + minutes
      end

      # [time, value] with time as minutes after midnight or an Anchor.
      def keyframe!(keyframe)
        unless keyframe.is_a?(Array) && keyframe.size == 2
          fail("keyframes must be [time, value] pairs, got #{keyframe.inspect}")
        end
        time, value = keyframe
        [keyframe_time!(time), number!(value, "keyframe value")]
      end

      # The adapter reference of a device from options like
      # `zigbee: "0x…", adapter: "z2m"`.
      def device_ref(options, what)
        kinds = ADAPTER_KINDS.select { |kind| options.key?(kind) }
        unless kinds.size == 1
          options_list = ADAPTER_KINDS.map { |kind| "#{kind}:" }.join(", ")
          fail("#{what} needs exactly one of #{options_list}")
        end
        kind = kinds.first
        device = options[kind]
        fail("#{what} #{kind}: must be a String id") unless non_empty_string?(device)
        adapter = options[:adapter]
        fail("adapter: must be a String") unless adapter.nil? || adapter.is_a?(String)
        { adapter_kind: kind.to_s, adapter: adapter, device: device }
      end

      def reject_unknown_options!(options, allowed, what)
        unknown = options.keys - allowed
        return if unknown.empty?
        fail("unknown option(s) for #{what}: #{unknown.join(", ")}")
      end

      # The "Room/Entity" paths among `targets`, which may contain lists.
      def paths(targets)
        paths = targets.flatten
        paths.each do |path|
          unless path.is_a?(String) && path.include?("/")
            raise ConfigError, "expected a \"Room/Entity\" path, got #{path.inspect}"
          end
        end
        paths
      end

      # The paths of every entity of the given kinds, e.g. [:groups].
      def entity_paths(kinds)
        model[:rooms].flat_map do |room|
          kinds.flat_map do |kind|
            room[kind].map { |entity| "#{room[:name]}/#{entity[:name]}" }
          end
        end
      end

      def non_empty_string?(value)
        value.is_a?(String) && !value.empty?
      end

      # mruby has no Regexp, so digits are checked one by one.
      def digits?(text)
        !text.empty? && text.each_char.all? { |char| char >= "0" && char <= "9" }
      end

      private

      def config_frame?(line)
        prelude = line.start_with?(PRELUDE_FILE)
        mruby_library = line.include?("/mrblib/")
        !prelude && !mruby_library
      end

      # "HH:MM" with hours below 24 and minutes below 60.
      def clock_time?(value)
        return false unless value.is_a?(String) && value.size == 5 && value[2] == ":"
        hours = value[0, 2]
        minutes = value[3, 2]
        return false unless digits?(hours + minutes)
        hours.to_i < 24 && minutes.to_i < 60
      end

      def keyframe_time!(time)
        case time
        when String then clock_minutes!(time, "keyframe time")
        when Symbol then Anchor.new(time, 0)
        when Anchor then time
        else
          fail("keyframe time must be \"HH:MM\", :wake, :bed or e.g. :bed - 1.h, " \
               "got #{time.inspect}")
        end
      end
    end
  end
end
