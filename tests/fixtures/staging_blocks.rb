# Two groups with staging blocks: one that picks by time and need, and one
# that returns a single light instead of a list.

zigbee "z2m", mqtt: "mqtt://x"

room "A" do
  light "a", zigbee: "1"
  light "b", zigbee: "2"

  group "G", lights: [:a, :b] do
    staging { |need, now| now.hour < 9 || need > 0.5 ? [:a, :b] : [:b] }
  end

  group "H", lights: [:a, :b] do
    staging { |need, now| :b }
  end
end
