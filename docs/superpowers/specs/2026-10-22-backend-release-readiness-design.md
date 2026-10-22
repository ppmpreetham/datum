# Datum Backend Release Readiness Design

## Objective

Make Datum's native Windows Rust backend release ready for ArduPilot setup and
calibration. Preserve the existing Tauri command names and frontend behavior
while improving reliability, observability, API discovery, documentation, and
test coverage.

Motor testing remains unchanged. Physical motor operation is outside this
work because it has not been validated on hardware.

## Current state

The active backend is implemented mainly in `src-tauri/src/calibration.rs`.
Frame selection, accelerometer calibration, compass calibration, radio
capture, ESC guidance, flight modes, failsafes, battery monitoring, and motor
testing are registered as Tauri commands.

Hardware tests established that serial connection, six-position
accelerometer calibration, and two-compass calibration work with the connected
ArduPilot controller. The remaining release issues are missing persistent
logging, stale API documentation, an all-target build failure, discarded
transport errors, overly broad radio channel validation, and a large backend
module with duplicated transport logic.

## Compatibility

- Existing registered Tauri command names remain available.
- Existing request and response fields remain compatible unless a field is
  explicitly documented as new and optional.
- New discovery endpoints provide labels, numeric firmware values, valid
  ranges, units, and compatibility information.
- Backend-only work must not modify frontend source files.
- The application remains native Windows software and does not use Docker.

## Module structure

Split the active backend into focused modules under `src-tauri/src/calibration/`:

- `mod.rs`: service construction, shared state, common DTOs, and command exports.
- `connection.rs`: serial discovery, connection lifecycle, heartbeat handling,
  vehicle identity, and transport errors.
- `worker.rs`: the single MAVLink worker and command dispatch loop.
- `parameters.rs`: parameter request, write, retry, missing-value tracking, and
  verified readback.
- `frame.rs`: frame discovery, current configuration, validation, and writes.
- `accelerometer.rs`: six-position controller-driven calibration state.
- `compass.rs`: onboard calibration, firmware progress, reports, and guidance.
- `radio.rs`: selected-channel capture, range validation, parameter generation,
  and verified saving.
- `esc.rs`: protocol discovery and the manually confirmed ESC workflow.
- `vehicle_config.rs`: flight modes, failsafes, battery configuration, and
  metadata.
- `motor.rs`: the existing guarded motor-test implementation without behavior
  expansion.

Shared transport helpers construct MAVLink messages and return send errors.
Pending operations retain their retry and timeout state in explicit types.

The existing unused parallel modules (`backend.rs`, `drone.rs`, and
`mavlink.rs`) will either be removed or reduced to code genuinely used by the
active backend. The `mavlink_monitor` binary will import supported modules and
must compile as part of an all-target check.

## Logging

Add a persistent native logger initialized during Tauri startup. Logs are
written to the operating system application log directory and mirrored to the
development console.

Every record contains:

- timestamp
- severity
- event name
- connection generation or calibration session when available
- vehicle system and component IDs when available
- a human-readable outcome or error

Log these events:

- application and WebSocket startup or failure
- connection request, serial open result, heartbeat identification,
  disconnection, heartbeat loss
- calibration start, acknowledgement, requested step, progress, completion,
  cancellation, timeout, and failure
- parameter request, retry, write, verified readback, mismatch, and timeout
- motor-test request validation and controller acknowledgement, without adding
  new motor behavior
- Tauri event emission failures

Avoid logging secrets or unnecessary full parameter payloads. Parameter names
and operation outcomes are acceptable.

Logging initialization failure must return a startup error rather than panic.

## WebSocket service

- Bind the gyro WebSocket to `127.0.0.1:8000` by default.
- Replace listener and server `unwrap()` calls with logged errors.
- Keep `/ws/gyro` compatible with the current frontend.
- Treat receiver disconnects as normal debug-level events.

## Parameter operations

All MAVLink sends used by a requested operation must be checked. An immediate
send failure returns a transport error to the caller and records a log event.
Retries resend only missing parameters. Verified writes complete only when
firmware readback matches every expected value.

Concurrent parameter operations remain serialized. Busy responses identify
the active operation rather than silently timing out.

Mutex poisoning must be converted into a stable backend-state error wherever
it can reach a Tauri command. Worker failure must update vehicle state and
release pending callers with errors.

## Discovery APIs

Add these Tauri commands:

### `flight_mode_options`

Returns supported ArduCopter mode values with IDs, labels, and descriptions,
plus valid mode-channel bounds.

### `failsafe_options`

Returns separately scoped options for throttle, battery-low,
battery-critical, and GCS failsafes. Each option includes its numeric firmware
value and label. Options must not imply compatibility beyond the supported
ArduPilot range documented by Datum.

### `battery_monitor_options`

Returns supported monitor types and validation metadata for capacity, voltage
multiplier, amps-per-volt, low voltage, critical voltage, and measured-voltage
calibration.

### `radio_calibration_options`

Returns channels currently reported by the receiver, whether each has a live
signal, parameter names that will be written, minimum required travel, and
trim behavior.

### `esc_protocol_options`

Returns known `MOT_PWM_TYPE` values, labels, and whether traditional endpoint
calibration applies. It also states that completion is manually confirmed.

The existing `frame_options` command remains the frame discovery API.

## Radio calibration

Extend the start request with an optional `channels` array. When omitted,
Datum captures channels with a valid live signal. When supplied, every channel
must be between 1 and 18 and must have live telemetry.

Only selected channels participate in minimum/maximum validation and parameter
writes. Unused fixed receiver channels cannot block completion. The backend
continues to require at least 400 microseconds of travel for selected channels
when range capture is enabled.

Trim is captured at calibration start. The response and documentation state
this behavior so the frontend can instruct the user to center roll, pitch, and
yaw, keep throttle low, and place switches in their neutral positions before
starting.

Saving writes only the selected `RCx_MIN`, `RCx_MAX`, and requested
`RCx_TRIM` parameters, then verifies them through readback.

## Compass progress and guidance

ArduPilot supplies completion percentage and a completion mask but does not
request a named X, Y, or Z rotation. Datum must not present a generated axis as
a firmware instruction.

Compass status gains a `guidance` object containing:

- `text`: movement advice selected from current progress and recent progress
  change
- `source`: `backend_heuristic`
- `progress_percent`: the firmware percentage used to choose the advice

Guidance changes across progress bands and when progress stalls. It suggests
varying orientation and rotating through uncovered attitudes. It never claims
that ArduPilot requested a particular axis. Firmware percentage, fitness,
success, and saved state remain authoritative.

Polling remains a frontend concern: the frontend may call
`compass_calibration_status` once per second. The command returns the most
recent backend state without generating extra MAVLink traffic.

## ESC calibration

ESC calibration remains a protocol-aware manual workflow because tones and
power-cycle state cannot be verified through the active USB MAVLink
connection.

- DShot and brushed protocols remain rejected for traditional endpoint
  calibration.
- Each advance request must match the current active step.
- Completed or cancelled workflows cannot advance.
- Responses explicitly report `verification: manually_confirmed`.

## Flight modes, failsafes, and battery

Existing read and verified-write commands remain unchanged. The new metadata
commands provide frontend labels, numeric choices, units, and validation
ranges. Validation uses shared metadata so the options returned to the
frontend cannot drift from write validation.

Battery monitoring continues to combine configured `BATT_*` parameters with
live `SYS_STATUS` voltage, current, and remaining percentage. Unavailable
telemetry remains `null` rather than being fabricated.

## Endpoint manifest and documentation

Generate or maintain one canonical manifest matching the commands registered
in `lib.rs`. Remove unavailable legacy command descriptions. The manifest must
state which commands read hardware, write firmware parameters, require a
disarmed vehicle, or rely on manual confirmation.

Update all files under `src-tauri/docs/` and `FRONTEND_INTEGRATION.md` with:

- exact Tauri command names
- complete request and response JSON
- enum values, ranges, and units
- retry and timeout behavior
- firmware and disarm requirements
- manual-verification limitations

## Error handling

Errors use stable prefixes where the frontend needs programmatic handling,
including:

- `BACKEND_STATE_ERROR`
- `VEHICLE_NOT_CONNECTED`
- `PARAMETER_OPERATION_BUSY`
- `MAVLINK_SEND_FAILED`
- `PARAMETER_TIMEOUT`
- `PARAMETER_VERIFICATION_FAILED`
- `INVALID_REQUEST`
- `UNSUPPORTED_FEATURE`

Human-readable details follow the prefix. Existing error prefixes remain
accepted where changing them would break compatibility.

## Testing and verification

Add unit tests for:

- discovery metadata and validation sharing the same definitions
- selected and unused radio channel handling
- compass guidance changes and heuristic labelling
- ESC terminal states
- motor command construction without executing a motor
- parameter missing-value and retry selection
- logging record formatting and initialization errors
- WebSocket bind error handling
- endpoint manifest parity with registered commands

Verification commands:

1. `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
2. `cargo test --manifest-path src-tauri/Cargo.toml --lib`
3. `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`
4. `cargo build --manifest-path src-tauri/Cargo.toml --bin datum`
5. `git diff --check`

Windows Application Control currently blocks some newly generated executables
intermittently. A blocked test or launch must be reported separately from a
Rust compile failure. Completion requires a successful native build; hardware
claims remain limited to the already tested accelerometer and compass flows.

## Completion criteria

- All active commands are documented in the canonical manifest.
- No documented command is absent from the Tauri handler.
- Operational logs persist connection, calibration, parameter, and service
  failures.
- Unused radio channels do not block selected-channel calibration.
- Compass guidance is dynamic and explicitly heuristic.
- MAVLink send errors reach callers promptly.
- The WebSocket service fails safely and binds locally.
- All Rust targets compile.
- Existing frame, accelerometer, compass, ESC, radio, flight mode, failsafe,
  battery, and motor API compatibility is preserved.

