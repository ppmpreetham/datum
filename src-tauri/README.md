# Datum Rust backend

The backend owns the drone's serial connection and exposes stable Tauri commands to the frontend. It detects ArduPilot or PX4 from the MAVLink heartbeat, sends firmware-specific calibration commands, keeps the current calibration state, and streams live progress over WebSocket.

The frontend implementation contract, TypeScript payloads, integration order, and magnetometer examples are in [FRONTEND_INTEGRATION.md](FRONTEND_INTEGRATION.md). The complete machine-readable API is in [endpoints.json](endpoints.json).

## Data flow

```text
Flight controller
  <-> USB serial / MAVLink
Rust DroneState
  -> Tauri invoke snapshots and control commands
  -> ws://127.0.0.1:8000/ws/calibration (live calibration events)
  -> ws://127.0.0.1:8000/ws/gyro (attitude and phone-orientation relay)
Frontend
```

Only one application should own the controller's serial port. Disconnect Mission Planner or QGroundControl before Datum connects.

## Development

From the repository root:

```bash
pnpm tauri dev
```

Backend verification from `src-tauri`:

```bash
cargo fmt -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

In an environment where the project filesystem is mounted `noexec`, set an executable target directory, for example `CARGO_TARGET_DIR=/tmp/datum-cargo-target`.
