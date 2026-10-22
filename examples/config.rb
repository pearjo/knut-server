# Example Knut configuration. Validate with `knut check examples/config.rb`.

location lat: 52.99, lon: 10.58

homekit name: "Knut"

hue    "bridge", host: "192.168.1.20"
zigbee "z2m",    mqtt: "mqtt://127.0.0.1:1883"

room "Küche" do
  light "spot1", zigbee: "0x0000000000000001"
  light "spot2", zigbee: "0x0000000000000002"
  light "spot3", zigbee: "0x0000000000000003"
  light "spot4", zigbee: "0x0000000000000004"

  group "Spots", lights: [:spot1, :spot2, :spot3, :spot4] do
    map :spot3, brightness: ->(x) { x * 0.8 }
  end
end

room "Wohnzimmer" do
  light "Licht", hue: "00000000-0000-0000-0000-000000000001"
end

room "Flur" do
  light  "Licht",    zigbee: "0x0000000000000010"
  sensor "Bewegung", zigbee: "0x0000000000000011"
end

scene "Filmabend", auto: false do
  set "Wohnzimmer/Licht", brightness: 20, kelvin: 2200
  turn_off "Küche/Spots"
end

scene "Morgen", auto: true do
  turn_on "Küche/Spots", "Flur/Licht"
end

on "Clock", at: "06:30" do
  activate "Morgen"
end

on "Sun", :sunset, offset: -30 do
  turn_on "Wohnzimmer/Licht"
end

on "Flur/Bewegung", :motion do |event, now|
  set "Flur/Licht", brightness: now.hour < 6 ? 5 : 60 unless on?("Flur/Licht")
end

on "Flur/Bewegung", :clear do
  turn_off "Flur/Licht", transition: 10
end

on "Sun", :sun do |sun, now|
  adapt all_groups, kelvin: sun.elevation > 10 ? 4000 : 2400, transition: 60
end
