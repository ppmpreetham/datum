# Current task

Status: completed in working tree on 2026-10-11.

Goal: inspect Datum, design the backend pipeline, implement Rust MAVLink calibration support with a magnetometer API and live calibration events, and provide a frontend integration README without changing frontend source files.

Acceptance results:

- ArduPilot and PX4 serial connections are heartbeat-verified.
- Magnetometer calibration uses the correct firmware-specific MAVLink commands.
- Calibration progress is retained and streamed at `/ws/calibration`.
- ArduPilot start options, accept, and cancel calls are documented.
- Frontend source under `src/` was not changed.
- Rust formatting, compile checks, tests, and strict lint were run; see `STATE.md` for results.
- Hardware-in-the-loop verification remains pending because no flight controller was connected.
