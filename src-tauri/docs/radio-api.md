# Radio calibration API

Datum captures radio endpoints from live ArduPilot `RC_CHANNELS` telemetry and
saves them through the application's managed MAVLink connection. Live channel
updates are emitted as `datum://radio`.

## Commands

- `radio_channels` returns current RC values and captured ranges.
- `start_radio_calibration` starts capture and requests `RC_CHANNELS` at 10 Hz.
- `radio_calibration_status` returns the current session.
- `finish_radio_calibration` validates, writes, and reads back `RCx_MIN`,
  `RCx_MAX`, and optionally `RCx_TRIM`.
- `cancel_radio_calibration` stops capture without writing parameters.

## Start request

Center roll, pitch, and yaw, set throttle low, and place switches in their
intended neutral positions before starting if trim capture is enabled.

```json
{
  "request": {
    "capture_min_max": true,
    "capture_trim": true
  }
}
```

Move every relevant stick, knob, and switch through its full range. Datum
rejects captured ranges narrower than 400 microseconds.

## Status

```json
{
  "session_id": "radio-1-123456",
  "status": "capturing",
  "signal_available": true,
  "channels": [
    {
      "channel": 1,
      "value_us": 1500,
      "minimum_us": 1000,
      "maximum_us": 2000,
      "trim_us": 1500,
      "range_valid": true
    }
  ]
}
```

Values come from `RC_CHANNELS`. Zero and `UINT16_MAX` are unavailable values.

## Finish or cancel

```json
{ "request": { "session_id": "radio-1-123456" } }
```

Finish waits for parameter write responses, requests every parameter again,
and succeeds only when readback matches. Datum does not use
`MAV_CMD_PREFLIGHT_CALIBRATION` for ArduPilot endpoint capture because that
command does not collect transmitter ranges.
