# Datum architecture

## Runtime boundary

- `src/`: React/Vite frontend maintained separately from this backend task.
- `src-tauri/`: Rust/Tauri backend and its frontend contract.
- Tauri `invoke` commands carry control requests and snapshots.
- Axum on `0.0.0.0:8000` carries live WebSocket events.

## Backend modules

- `src-tauri/src/lib.rs`: Tauri startup, managed drone state, command registration, and WebSocket server startup.
- `src-tauri/src/drone.rs`: serial discovery, verified MAVLink connection, shared status/telemetry/calibration state, incoming message handling, and command sending.
- `src-tauri/src/calibration.rs`: feature-gated calibration APIs and ArduPilot/PX4 command selection.
- `src-tauri/src/server.rs`: `/ws/gyro` attitude relay and read-only `/ws/calibration` event stream.
- `src-tauri/src/backend.rs`: backend metadata and compiled endpoint-manifest access.
- `src-tauri/src/mavlink.rs` and `src-tauri/src/bin/mavlink_monitor.rs`: backend diagnostic parser and command-line monitor.

## Calibration flow

1. Frontend opens `/ws/calibration` and calls a serial connection command.
2. Backend waits for a supported heartbeat and records system/component IDs and flight stack.
3. Frontend starts one calibration through a Tauri command.
4. Backend sends the appropriate MAVLink command and stores an active session.
5. Incoming `COMMAND_ACK`, `STATUSTEXT`, accelerometer-position, `MAG_CAL_PROGRESS`, and `MAG_CAL_REPORT` messages update retained state and emit live calibration events.
6. Frontend uses the WebSocket for immediate display and Tauri snapshot commands for authoritative recovery after startup or reconnect.

ArduPilot magnetometer calibration uses `MAV_CMD_DO_START_MAG_CAL`, optional accept/cancel commands, and `MAG_CAL_PROGRESS`/`MAG_CAL_REPORT`. PX4 magnetometer calibration uses `MAV_CMD_PREFLIGHT_CALIBRATION` and its command/status-text progress protocol.
