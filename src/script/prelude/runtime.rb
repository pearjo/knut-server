# Runtime: the methods Rust calls once the config has loaded. Each returns
# JSON; see src/script/mod.rs for the other side.

module Knut
  class << self
    # The recorded model of the home.
    def model_json
      Json.encode(Internal.model)
    end

    # Runs the scene or trigger block `id` with `args`, where `state` is a
    # snapshot of every entity keyed by path. Returns the actions it asked for.
    def run(id, args, state)
      block = Internal.registered_proc(id)
      actions = Internal.collecting_actions(state) { block.call(*args) }
      Json.encode(actions)
    end

    # The brightness mapping `id` evaluated for every brightness 1..100.
    def mapping_table(id)
      mapping = Internal.registered_proc(id)
      table = (1..100).map { |x| mapped_brightness(mapping, x) }
      Json.encode(table)
    end

    # Runs the staging block `id` with `need` (0–1) and the Unix time `now`.
    # Returns the names of the lights it picked.
    def staging(id, need, now)
      lights = Internal.registered_proc(id).call(need, Time.at(now))
      unless lights.is_a?(Array)
        raise ConfigError,
              "staging block returned #{lights.inspect}, expected an Array of lights like [:spot1]"
      end
      Json.encode(lights.map(&:to_s))
    end

    private

    def mapped_brightness(mapping, x)
      value = mapping.call(x)
      unless value.is_a?(Numeric)
        raise ConfigError,
              "brightness mapping returned #{value.inspect} for #{x}, expected a number"
      end
      value.to_f
    end
  end
end
