# ESC calibration API

The ESC backend reads ArduPilot `MOT_PWM_TYPE` and only offers traditional
endpoint calibration for compatible PWM protocols. DShot and brushed outputs
are rejected because the guided PWM endpoint procedure does not apply.

Commands: `esc_capabilities`, `esc_steps`, `start_esc_calibration`,
`advance_esc_calibration`, `esc_calibration_status`, and
`cancel_esc_calibration`.

Starting requires `{ "request": { "propellers_removed": true } }`. Each step
is advanced with a session ID, the current step ID, and action
`confirm_completed`. Completion remains manually confirmed because ESC tones
and power-cycle behavior cannot be verified through the active MAVLink USB
connection.
