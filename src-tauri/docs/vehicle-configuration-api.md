# Flight modes, failsafes, and battery API

All commands use Tauri IPC and the single managed MAVLink connection. Writes
require a disarmed vehicle and are followed by explicit parameter readback.

## Flight modes

- `flight_mode_configuration` reads `FLTMODE1` through `FLTMODE6`,
  `FLTMODE_CH`, and the active mode from `HEARTBEAT.custom_mode`.
- `set_flight_mode_configuration` accepts `mode_channel` and exactly six mode
  values. It changes switch-slot configuration without commanding a live mode.

## Failsafes

- `failsafe_configuration` reads throttle, battery-low, battery-critical, and
  GCS failsafe actions.
- `set_failsafe_configuration` validates and writes `FS_THR_ENABLE`,
  `BATT_FS_LOW_ACT`, `BATT_FS_CRT_ACT`, and `FS_GCS_ENABLE`.

## Battery monitoring

- `battery_configuration` returns monitor type, capacity, scaling,
  low/critical thresholds, and live `SYS_STATUS` voltage/current/remaining.
- `set_battery_configuration` validates and writes the `BATT_*` parameters.
- `calibrate_battery_voltage` accepts a trusted multimeter voltage and adjusts
  `BATT_VOLT_MULT` by `old_multiplier * measured / telemetry`.

Battery voltage calibration is rejected without valid live telemetry. Capacity
and voltage thresholds must be positive, and the low threshold must remain
above the critical threshold.
