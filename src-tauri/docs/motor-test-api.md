# Motor test API

Command: `run_motor_test`

```json
{
  "request": {
    "motor_number": 1,
    "throttle_percent": 5.0,
    "duration_seconds": 1.0,
    "propellers_removed": true
  }
}
```

The backend requires a connected, disarmed ArduPilot vehicle and explicit
propeller-removal confirmation. It limits output to 1-15 percent and duration
to 0.1-5 seconds, sends `MAV_CMD_DO_MOTOR_TEST`, and reports success only after
an accepted or in-progress `COMMAND_ACK`. A timeout returns an unknown result.

Motor numbering follows the flight controller's configured motor order.
