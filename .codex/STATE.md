# Datum project state

Updated: 2026-10-11

Datum is a Tauri 2 desktop application for drone calibration. The frontend is React/Vite/TypeScript and the backend is Rust under `src-tauri`.

The backend now supports serial discovery and connection, ArduPilot/PX4 heartbeat detection, selected telemetry, sensor calibration commands, retained calibration state, and live WebSocket events. Magnetometer calibration uses the flight-stack-specific MAVLink protocol.

Current task status: completed in the working tree; not committed.

Verification:

- `cargo fmt -- --check`: passed.
- `CARGO_TARGET_DIR=/tmp/datum-cargo-target cargo check --all-targets --offline`: passed.
- `CARGO_TARGET_DIR=/tmp/datum-cargo-target cargo test --all-targets --offline`: passed, 17 tests.
- JSON parsing for `src-tauri/endpoints.json` and `src-tauri/calibration_sensors.json`: passed.
- Hardware-in-the-loop calibration: not run because no flight controller was connected for this task.

Known constraints:

- The project filesystem does not allow executing Cargo build scripts from `src-tauri/target`; use a target directory on an executable filesystem such as `/tmp/datum-cargo-target` in this environment.
- Mission Planner, QGroundControl, and Datum generally cannot own the same serial port simultaneously.
