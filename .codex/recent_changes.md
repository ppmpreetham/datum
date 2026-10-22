# Recent changes

## 2026-10-11

- Added Rust serial discovery, verified MAVLink connections, ArduPilot/PX4 detection, telemetry snapshots, and calibration session tracking.
- Added flight-stack-specific accelerometer, gyroscope, magnetometer, radio, and level-horizon calibration commands.
- Added ArduPilot magnetometer options plus accept/cancel control and multi-compass result tracking.
- Added a read-only live calibration stream at `/ws/calibration`; retained `/ws/gyro` for attitude and phone-orientation data.
- Added `src-tauri/README.md`, `src-tauri/endpoints.json`, and `src-tauri/FRONTEND_INTEGRATION.md` for backend and frontend integration.
- Verified 17 Rust tests, all-target compilation, JSON syntax, formatting, strict Clippy, and diff whitespace. Hardware calibration was not run.
