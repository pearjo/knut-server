# A small home with every kind of entity and trigger, for the script tests.

location lat: 52.99, lon: 10.58

hue    "bridge", host: "192.168.1.20"
zigbee "z2m",    mqtt: "mqtt://localhost:1883"

room "Kitchen" do
  light "spot1",   zigbee: "0x01"
  light "spot2",   zigbee: "0x02"
  light "spot3",   zigbee: "0x03"
  light "ceiling", hue: "4f3c"

  group "Spots", lights: [:spot1, :spot2, :spot3] do
    map :spot3, brightness: ->(x) { x * 0.8 }
  end
end

room "Hallway" do
  light  "Light",  zigbee: "0x10"
  sensor "Motion", zigbee: "0x11"
end

scene "Movie Night", auto: false do
  set "Kitchen/Spots", brightness: 20, kelvin: 2200
end

scene "Morning", auto: true do
  turn_on "Kitchen/Spots", "Hallway/Light"
end

on "Clock", at: "06:30" do
  activate "Morning"
end

on "Hallway/Motion", :motion do |event, now|
  turn_on "Hallway/Light" unless on?("Hallway/Light")
end

on "Sun", :sun do |sun, now|
  adapt all_groups, kelvin: sun.elevation > 10 ? 4000 : 2400, transition: 60
end
