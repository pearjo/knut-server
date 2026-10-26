# A demand curve anchored on a schedule with late weekends, for the curve
# tests. `$demand` is global so that the tests can evaluate it.

schedule do
  weekdays wake: "06:30", bed: "23:00"
  friday   bed: "00:30"
  saturday wake: "08:30", bed: "00:30"
  sunday   wake: "08:30", bed: "23:00"
end

$demand = curve(
  [:bed + 1.h,   0.0],
  [:wake,        0.0],
  [:wake + 30.m, 1.0],
  ["17:00",      1.0],
  [:bed - 3.h,   0.6],
  [:bed - 1.h,   0.25])
