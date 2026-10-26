# The home the engine tests run against: a mapped group with daylight, a
# hallway with a motion trigger, and a bathroom with staged groups.

zigbee "z2m", mqtt: "mqtt://localhost"

room "Kitchen" do
  daylight 0.5, facing: :south

  light "spot1", zigbee: "0x01"
  light "spot2", zigbee: "0x02"

  group "Spots", lights: [:spot1, :spot2] do
    map :spot2, brightness: ->(x) { x / 2 }
  end
end

room "Hallway" do
  light  "Light",  zigbee: "0x10"
  sensor "Motion", zigbee: "0x11"
end

room "Bathroom" do
  light "a", zigbee: "0x20"
  light "b", zigbee: "0x21"
  light "c", zigbee: "0x22"

  group "Light", lights: [:a, :b, :c] do
    stages [:b], [:a, :c]
  end

  group "Mirror", lights: [:a, :b] do
    staging { |need, now| need > 0.5 ? [:a, :b] : [:a] }
  end

  # A staging block that names a light outside the group.
  group "Tub", lights: [:b, :c] do
    staging { |need, now| [:x] }
  end
end

scene "Bright", auto: false do
  set "Kitchen/Spots", brightness: 90
end

scene "Morning", auto: true do
  turn_on "Kitchen/Spots"
end

scene "Chain", auto: false do
  activate "Bright"
end

on "Hallway/Motion", :motion do |event, now|
  turn_on "Hallway/Light"
end

on "Clock", at: "06:30" do
  turn_on "Kitchen/Spots"
end
