# Compass calibration API

Datum uses ArduPilot onboard compass calibration through the application's
single MAVLink connection. Live updates are emitted as `datum://compass`.

## Commands

- `compass_sensors` returns sensors detected from vehicle health and any
  individual compass IDs reported during calibration.
- `start_compass_calibration` sends `MAV_CMD_DO_START_MAG_CAL`.
- `compass_calibration_status` returns the latest firmware progress and report.
- `accept_compass_calibration` sends `MAV_CMD_DO_ACCEPT_MAG_CAL`.
- `cancel_compass_calibration` sends `MAV_CMD_DO_CANCEL_MAG_CAL`.
- `retry_compass_calibration` starts the same session settings again.

## Start request

```json
{
  "request": {
    "compass_ids": [0],
    "retry_on_failure": false,
    "autosave": false
  }
}
```

An empty `compass_ids` array sends mask zero, which asks ArduPilot to calibrate
all available compasses. IDs 0 through 7 are accepted and converted to the
MAVLink compass mask.

## Status response

```json
{
  "session_id": "compass-1-123456",
  "status": "in_progress",
  "instruction": "Rotate the drone as instructed until compass calibration finishes",
  "sensors": [
    {
      "compass_id": 0,
      "progress_percent": 65,
      "calibration_state": "running",
      "fitness": null,
      "fitness_source": "not_available",
      "saved": false,
      "failure_reason": null
    }
  ]
}
```

Progress comes directly from `MAG_CAL_PROGRESS.completion_pct`. Fitness and
failure state come directly from `MAG_CAL_REPORT`. Datum does not create a
quality score from elapsed time or completion percentage. Until a report is
received, fitness remains `null`.

Accept, cancel, and retry requests contain the session ID:

```json
{ "request": { "session_id": "compass-1-123456" } }
```
