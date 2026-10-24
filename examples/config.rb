# Example Knut configuration. Validate with `knut check examples/config.rb`.

location lat: 52.99, lon: 10.58

homekit name: "Knut"

hue    "bridge", host: "192.168.1.20"
zigbee "z2m",    mqtt: "mqtt://127.0.0.1:1883"

schedule do
  weekdays wake: "06:30", bed: "23:00"
  friday   bed: "00:30"
  saturday wake: "08:30", bed: "00:30"
  sunday   wake: "08:30", bed: "23:00"
end

# How much artificial light is wanted (0–1) before daylight is subtracted.
demand = curve(
  [:bed + 1.h,   0.0],
  [:wake,        0.0],
  [:wake + 30.m, 1.0],
  ["17:00",      1.0],
  [:bed - 3.h,   0.6],
  [:bed - 1.h,   0.25])

# Colour temperature in kelvin.
warmth = curve(
  [:bed + 1.h,   2200],
  [:wake,        2200],
  [:wake + 45.m, 4000],   # quickly cool to start the day
  ["15:00",      3000],   # gradually warmer
  [:bed - 3.h,   2800],   # evening plateau
  [:bed - 1.h,   2200])   # very warm for the late evening

room "Kitchen" do
  daylight 0.8, facing: :east

  light "spot1", zigbee: "0x0000000000000001"
  light "spot2", zigbee: "0x0000000000000002"
  light "spot3", zigbee: "0x0000000000000003"
  light "spot4", zigbee: "0x0000000000000004"

  group "Spots", lights: [:spot1, :spot2, :spot3, :spot4] do
    stages [:spot2, :spot3], [:spot1, :spot4]   # middle two first
    map :spot3, brightness: ->(x) { x * 0.8 }
  end
end

room "Living Room" do
  daylight 0.4, facing: [:south, :west]

  light "Light", hue: "00000000-0000-0000-0000-000000000001"
end

room "Hallway" do
  light  "Light",  zigbee: "0x0000000000000010"
  sensor "Motion", zigbee: "0x0000000000000011"
end

scene "Movie Night", auto: false do
  set "Living Room/Light", brightness: 20, kelvin: 2200
  turn_off "Kitchen/Spots"
end

scene "Morning", auto: true do
  turn_on "Kitchen/Spots", "Hallway/Light"
end

on "Clock", at: "06:30" do
  activate "Morning"
end

on "Sun", :sunset, offset: -30 do
  turn_on "Living Room/Light"
end

on "Hallway/Motion", :motion do |event, now|
  set "Hallway/Light", brightness: now.hour < 6 ? 5 : 60 unless on?("Hallway/Light")
end

on "Hallway/Motion", :clear do
  turn_off "Hallway/Light", transition: 10
end

on "Sun", :sun do |sun, now|
  adapt all_groups, "Living Room/Light", need: demand.at(now), kelvin: warmth.at(now), transition: 60
end
