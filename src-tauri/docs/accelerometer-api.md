# Accelerometer calibration API

Datum exposes accelerometer calibration through Tauri IPC over the application's
single managed MAVLink connection. Live updates are emitted as
`datum://accelerometer` events.

## Start

Command: `start_accelerometer_calibration`

```json
{ "calibrationType": "full" }
```

The backend requires a connected, disarmed ArduPilot or PX4 vehicle and sends
`MAV_CMD_PREFLIGHT_CALIBRATION`. It returns a generated session ID and the
initial controller-backed state.

## Confirm a requested position

Command: `accelerometer_calibration_step`

```json
{
  "request": {
    "session_id": "accel-1-123456",
    "step": "left",
    "action": "confirm_position"
  }
}
```

Valid controller-requested positions are `level`, `left`, `right`,
`nose_down`, `nose_up`, and `upside_down`. Datum rejects a step that does not
match the latest `MAV_CMD_ACCELCAL_VEHICLE_POS` request. Confirmation is sent
back using that MAVLink command and its actual position value.

## Status

Command: `accelerometer_calibration_status`

```json
{
  "session_id": "accel-1-123456",
  "status": "awaiting_user",
  "current_step": 2,
  "total_steps": 6,
  "orientation": "left",
  "instruction": "Place the drone on its left side, then press Continue",
  "waiting_for_confirmation": true,
  "percent": 17
}
```

The requested orientation comes from the flight controller; Datum does not
assume a fixed order.

## Cancel

Command: `cancel_accelerometer_calibration`

```json
{ "request": { "session_id": "accel-1-123456" } }
```

The backend sends `COMMAND_CANCEL` for the active calibration and reports the
actual controller acknowledgement through status and events. If the controller
does not acknowledge cancellation, the result remains unknown.
