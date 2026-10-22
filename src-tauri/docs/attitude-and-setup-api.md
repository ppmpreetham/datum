# Attitude and setup parameter API

These commands support the guided calibration UI. They use the application's
single managed MAVLink connection.

## Live attitude

On the first heartbeat the backend asks the controller for `ATTITUDE` at
25 Hz (`MAV_CMD_SET_MESSAGE_INTERVAL`) and asks again whenever the stream is
quiet for three seconds (for example after a reboot). Samples are emitted as
`datum://attitude` at up to ~30 Hz:

```json
{
  "roll": 0.01,
  "pitch": -0.02,
  "yaw": 1.57,
  "roll_speed": 0.0,
  "pitch_speed": 0.0,
  "yaw_speed": 0.0,
  "time_boot_ms": 123456
}
```

Angles are radians in the MAVLink NED convention. `vehicle_attitude` returns
the latest sample, or `null` before the first one arrives.

## Setup parameters

`read_vehicle_parameters` and `write_vehicle_parameters` read and write the
orientation and compass-selection parameters that QGroundControl asks about
before a sensor calibration. Only this allowlist is accepted:

| Parameter | Firmware | Meaning |
| --- | --- | --- |
| `AHRS_ORIENTATION` | ArduPilot | Autopilot (board) rotation |
| `COMPASS_EXTERNAL`, `COMPASS_EXTERN2`, `COMPASS_EXTERN3` | ArduPilot | Internal (0) / external (1) compass |
| `COMPASS_ORIENT`, `COMPASS_ORIENT2`, `COMPASS_ORIENT3` | ArduPilot | External compass rotation |
| `COMPASS_USE`, `COMPASS_AUTO_ROT` | ArduPilot | Compass use / auto-rotation detection |
| `SENS_BOARD_ROT` | PX4 | Autopilot (board) rotation |
| `CAL_MAG0_ROT` | PX4 | External magnetometer rotation |
| `MOT_PWM_TYPE` | ArduPilot | Read-only: ESC output protocol |

```ts
await invoke("read_vehicle_parameters", { names: ["AHRS_ORIENTATION"] });
// { "AHRS_ORIENTATION": 0 }

await invoke("write_vehicle_parameters", { parameters: { AHRS_ORIENTATION: 8 } });
```

Writes require a disarmed vehicle, accept only non-negative integers, and
return only after every value is read back from the controller.

## Gyro and level completion

ArduPilot runs gyro and level (`AHRS_TRIM`) calibration before it sends the
`COMMAND_ACK`, so an accepted acknowledgement for those two is reported as
`completed`, matching QGroundControl.

## Radio capture

`finish_radio_calibration` skips channels that never moved during capture
(less than 100 µs of travel), such as unused iBUS channels 7-14 on a FlySky
FS-iA6B. Those channels keep their existing parameters. A channel that moved
partway (100-399 µs) still fails the save.
