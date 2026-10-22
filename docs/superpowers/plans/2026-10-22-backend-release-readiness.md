# Datum Backend Release Readiness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Datum's native Windows calibration backend observable, reliably buildable, accurately documented, and safe for frontend integration while preserving existing Tauri command compatibility.

**Architecture:** Keep one managed MAVLink connection and worker, but move workflow and transport responsibilities out of the 3,700-line `calibration.rs` into focused modules behind a compatibility facade. Add persistent native logging, metadata APIs, selected-channel radio calibration, explicitly heuristic compass guidance, checked transport failures, and a canonical endpoint manifest.

**Tech Stack:** Rust 2021, Tauri 2, `mavlink` 0.17, Tokio/Axum, Serde, native Windows serial transport, Cargo tests.

**Spec:** `docs/superpowers/specs/2026-10-22-backend-release-readiness-design.md`

## Global Constraints

- Preserve every currently registered Tauri command name and compatible request/response field.
- Modify backend Rust, backend documentation, manifests, and tests only; do not modify frontend source.
- Keep Datum native on Windows without Docker.
- Keep a single managed MAVLink connection.
- Leave motor-test behavior unchanged except for logging and internal relocation.
- Treat firmware progress/readback as authoritative; label generated compass guidance `backend_heuristic`.
- Push each completed, verified task to `main` as requested by the user.

## Review Focus

- Windows Application Control may block a generated executable: distinguish policy error 4551 from compilation/test failures in every verification report (Task 1 and final verification).
- A receiver may report fixed unused channels: only explicitly selected live channels may block radio completion (Task 5 tests).
- A serial send may fail before a timeout begins: callers must receive `MAVLINK_SEND_FAILED` immediately (Task 3 tests).
- ArduPilot does not request named compass axes: guidance must remain heuristic and change on progress/stall without claiming firmware authority (Task 7 tests).
- Existing frontend calls may omit newly optional fields: deserialize old radio requests and preserve current response fields (Task 5 compatibility tests).

---

### Task 1: Establish a clean build and endpoint parity baseline

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/bin/mavlink_monitor.rs`
- Create: `src-tauri/src/endpoint_manifest.rs`
- Modify: `src-tauri/src/backend.rs`
- Modify: `src-tauri/endpoints.json`
- Test: `src-tauri/src/endpoint_manifest.rs`

**Interfaces:**
- Produces: `endpoint_manifest::registered_command_names() -> &'static [&'static str]`
- Produces: `endpoint_manifest::manifest_command_names(json: &str) -> Result<BTreeSet<String>, String>`
- Preserves: the `datum` binary and optional `mavlink_monitor` binary.

- [ ] **Step 1: Write failing manifest-parity and duplicate-command tests**

Assert that command names parsed from `src-tauri/endpoints.json` exactly equal `registered_command_names()` and contain no duplicates.

- [ ] **Step 2: Run the focused test and record the expected stale-manifest failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib endpoint_manifest`

Expected: FAIL because the existing JSON describes legacy commands and omits active commands; if Windows returns error 4551, record the policy block and use `cargo check --lib` to confirm compilation separately.

- [ ] **Step 3: Make the registered command list the shared manifest source**

Add `endpoint_manifest.rs`, update `lib.rs` to use the same command inventory in parity tests, and rewrite `endpoints.json` with every active command and its read/write/disarm/manual-confirmation flags.

- [ ] **Step 4: Repair `mavlink_monitor` imports without exposing unrelated private application internals**

Move the parser helpers it needs into an explicitly public library module or make the binary include a focused shared module. Remove stale unused backend entry points rather than registering a second connection stack.

- [ ] **Step 5: Verify all targets compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`

Expected: PASS with no unresolved `datum_lib::mavlink` or private `server` imports.

- [ ] **Step 6: Commit and push**

Commit message: `Fix backend target build and endpoint manifest`

### Task 2: Add persistent logging and safe WebSocket startup

**Files:**
- Create: `src-tauri/src/logging.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/server.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Test: `src-tauri/src/logging.rs`
- Test: `src-tauri/src/server.rs`

**Interfaces:**
- Produces: `logging::init(log_dir: &Path) -> Result<PathBuf, String>`
- Produces: `logging::format_record(level: Level, target: &str, message: &str, timestamp: SystemTime) -> String`
- Changes: `server::start() -> Result<(), String>` starts a task whose failures are logged.

- [ ] **Step 1: Write failing logger-format and initialization-error tests**

Assert stable timestamp/severity/target formatting, directory creation, append behavior, and a controlled error for an unwritable target.

- [ ] **Step 2: Write a failing WebSocket bind-address test**

Extract and assert the default address is `127.0.0.1:8000`; assert bind failures return/log errors rather than panic.

- [ ] **Step 3: Implement and initialize the native logger during Tauri setup**

Use the application log directory, mirror development logs to stderr, set an INFO default, and make initialization failure abort startup with a readable error.

- [ ] **Step 4: Replace WebSocket `unwrap()` calls and public binding**

Bind locally, log startup and service errors, and keep `/ws/gyro` unchanged.

- [ ] **Step 5: Run focused and library tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib logging`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib server`

Expected: PASS, subject to separately reported Windows policy blocking.

- [ ] **Step 6: Commit and push**

Commit message: `Add persistent backend logging`

### Task 3: Extract checked MAVLink parameter transport

**Files:**
- Create: `src-tauri/src/calibration/mod.rs`
- Create: `src-tauri/src/calibration/state.rs`
- Create: `src-tauri/src/calibration/parameters.rs`
- Create: `src-tauri/src/calibration/worker.rs`
- Modify: `src-tauri/src/calibration.rs` then remove it after facade migration
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/calibration/parameters.rs`

**Interfaces:**
- Produces: `BackendState::lock(&self) -> Result<MutexGuard<'_, Session>, String>` with `BACKEND_STATE_ERROR` on poison.
- Produces: `ParameterReadOperation` and `ParameterWriteOperation` types with `missing_names()`, `record_value()`, `should_retry()`, and `verification_result()`.
- Produces: checked worker send helpers returning errors prefixed `MAVLINK_SEND_FAILED`.
- Consumes: the logging API from Task 2.

- [ ] **Step 1: Write failing parameter-operation tests**

Cover missing-name selection, four retries, write/readback mismatch, exact successful readback, busy operations, and immediate transport-send failure.

- [ ] **Step 2: Write a failing poisoned-state test**

Poison a test mutex and assert `BackendState::lock()` returns `BACKEND_STATE_ERROR` without panicking.

- [ ] **Step 3: Extract shared state and parameter operations behind the existing Tauri facade**

Move state types first, then parameter retry/write verification. Preserve current outer command timeouts and response shapes.

- [ ] **Step 4: Replace discarded operation sends**

Check heartbeat-independent requested sends, message interval requests, parameter retries, calibration starts, and frame retries. Resolve each pending caller on terminal worker failure.

- [ ] **Step 5: Add connection and parameter lifecycle logs**

Log connection generation, vehicle IDs, operation type, parameter names, retry number, verification outcome, heartbeat loss, and transport errors without logging unnecessary values.

- [ ] **Step 6: Run parameter and connection tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib calibration::parameters`

Expected: PASS.

- [ ] **Step 7: Commit and push**

Commit message: `Extract reliable MAVLink parameter transport`

### Task 4: Extract frame and accelerometer workflows

**Files:**
- Create: `src-tauri/src/calibration/frame.rs`
- Create: `src-tauri/src/calibration/accelerometer.rs`
- Modify: `src-tauri/src/calibration/mod.rs`
- Modify: `src-tauri/src/calibration/worker.rs`
- Test: `src-tauri/src/calibration/frame.rs`
- Test: `src-tauri/src/calibration/accelerometer.rs`

**Interfaces:**
- Preserves commands: `frame_options`, `current_frame`, `set_frame`.
- Preserves commands: `start_accelerometer_calibration`, `accelerometer_calibration_status`, `accelerometer_calibration_step`, `cancel_accelerometer_calibration`.
- Consumes: checked send and parameter interfaces from Task 3.

- [ ] **Step 1: Move existing frame tests and add compatibility assertions**

Assert all existing frame IDs/numeric values, unknown-value preservation, invalid combinations, and verified writes retain the existing JSON field names.

- [ ] **Step 2: Extract frame code and run frame tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib calibration::frame`

Expected: PASS.

- [ ] **Step 3: Move existing accelerometer tests and add state-transition assertions**

Cover all six firmware positions, invalid session, wrong position, confirmation only while awaiting user, completion, cancellation, and logging events.

- [ ] **Step 4: Extract accelerometer code and run its tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib calibration::accelerometer`

Expected: PASS.

- [ ] **Step 5: Commit and push**

Commit message: `Extract frame and accelerometer workflows`

### Task 5: Make radio calibration channel-selective

**Files:**
- Create: `src-tauri/src/calibration/radio.rs`
- Modify: `src-tauri/src/calibration/mod.rs`
- Modify: `src-tauri/src/calibration/worker.rs`
- Modify: `src-tauri/docs/radio-api.md`
- Test: `src-tauri/src/calibration/radio.rs`

**Interfaces:**
- Extends: `RadioStartRequest { capture_min_max: bool, capture_trim: bool, channels: Option<Vec<u8>> }`.
- Produces: `selected_radio_channels(requested: Option<&[u8]>, live: &[RadioChannelStatus]) -> Result<BTreeSet<u8>, String>`.
- Preserves existing request compatibility when `channels` is omitted.

- [ ] **Step 1: Write failing selection and compatibility tests**

Assert omitted `channels` selects live channels, explicit selection accepts unique live channels 1–18, invalid/dead channels fail, and unused fixed channels do not prevent finishing selected channels.

- [ ] **Step 2: Write failing parameter-generation tests**

Assert only selected `RCx_MIN`, `RCx_MAX`, and requested `RCx_TRIM` names are generated; selected ranges below 400 microseconds fail.

- [ ] **Step 3: Extract and implement radio workflow**

Track selected channels in session state, preserve capture-at-start trim semantics, save only selected parameters, and verify readback through Task 3.

- [ ] **Step 4: Update radio API documentation**

Document optional channels, live-signal validation, trim timing, response JSON, errors, and verified writes.

- [ ] **Step 5: Run radio tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib calibration::radio`

Expected: PASS.

- [ ] **Step 6: Commit and push**

Commit message: `Support selected radio calibration channels`

### Task 6: Add configuration metadata and extract ESC/config workflows

**Files:**
- Create: `src-tauri/src/calibration/esc.rs`
- Create: `src-tauri/src/calibration/vehicle_config.rs`
- Modify: `src-tauri/src/calibration/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/endpoints.json`
- Test: `src-tauri/src/calibration/esc.rs`
- Test: `src-tauri/src/calibration/vehicle_config.rs`

**Interfaces:**
- Adds commands: `esc_protocol_options`, `flight_mode_options`, `failsafe_options`, `battery_monitor_options`, `radio_calibration_options`.
- Preserves existing ESC, flight mode, failsafe, battery, and voltage-calibration commands.
- Metadata definitions are the single source used by both option responses and write validation.

- [ ] **Step 1: Write failing metadata/validation parity tests**

Assert every accepted mode, failsafe value, battery range, and ESC protocol appears in its options response and every rejected value is absent or marked unsupported.

- [ ] **Step 2: Write ESC terminal-state and manual-verification tests**

Assert DShot/brushed rejection, ordered active steps, no advance after cancel/complete, and explicit `verification: manually_confirmed`.

- [ ] **Step 3: Extract ESC and vehicle configuration modules**

Move current logic without changing existing command responses, then add the metadata commands and shared validation tables.

- [ ] **Step 4: Register new commands and update the canonical manifest**

Add all five discovery commands to `lib.rs`, shared command inventory, and `endpoints.json`.

- [ ] **Step 5: Run workflow and parity tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib calibration::esc`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib calibration::vehicle_config`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib endpoint_manifest`

Expected: PASS.

- [ ] **Step 6: Commit and push**

Commit message: `Add calibration configuration metadata APIs`

### Task 7: Add dynamic heuristic compass guidance

**Files:**
- Create: `src-tauri/src/calibration/compass.rs`
- Modify: `src-tauri/src/calibration/mod.rs`
- Modify: `src-tauri/src/calibration/worker.rs`
- Modify: `src-tauri/docs/compass-api.md`
- Test: `src-tauri/src/calibration/compass.rs`

**Interfaces:**
- Adds: `CompassGuidance { text: String, source: &'static str, progress_percent: u8 }`.
- Extends: `CompassCalibrationStatus` with `guidance: Option<CompassGuidance>`.
- Produces: `compass_guidance(progress: u8, previous_progress: u8, stalled_updates: u8) -> CompassGuidance`.

- [ ] **Step 1: Write failing guidance tests**

Assert guidance changes across progress bands, changes after repeated stalled updates, always reports `backend_heuristic`, preserves firmware percentage, and never contains wording that claims ArduPilot requested an axis.

- [ ] **Step 2: Move existing compass mask/report tests**

Retain multi-compass success, firmware fitness, autosave, retry, accept, cancel, and failed-report behavior.

- [ ] **Step 3: Extract compass workflow and add guidance state**

Update guidance from received `MAG_CAL_PROGRESS`; status polling reads state only and sends no MAVLink request.

- [ ] **Step 4: Update compass documentation**

Document one-second polling, guidance JSON, heuristic limitations, authoritative firmware fields, and multi-compass completion.

- [ ] **Step 5: Run compass tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib calibration::compass`

Expected: PASS.

- [ ] **Step 6: Commit and push**

Commit message: `Add dynamic compass calibration guidance`

### Task 8: Finish module extraction and documentation synchronization

**Files:**
- Create: `src-tauri/src/calibration/motor.rs`
- Modify: `src-tauri/src/calibration/mod.rs`
- Modify: `src-tauri/src/calibration/worker.rs`
- Remove: obsolete `src-tauri/src/calibration.rs`
- Remove or integrate: `src-tauri/src/backend.rs`, `src-tauri/src/drone.rs`, `src-tauri/src/mavlink.rs`
- Modify: `src-tauri/FRONTEND_INTEGRATION.md`
- Modify: `src-tauri/docs/accelerometer-api.md`
- Modify: `src-tauri/docs/esc-api.md`
- Modify: `src-tauri/docs/frame-api.md`
- Modify: `src-tauri/docs/motor-test-api.md`
- Modify: `src-tauri/docs/serial-api.md`
- Modify: `src-tauri/docs/vehicle-configuration-api.md`
- Test: `src-tauri/src/endpoint_manifest.rs`

**Interfaces:**
- Preserves: existing motor request limits, propeller confirmation, ArduPilot/disarmed requirements, and ACK result.
- Produces: one active backend module tree and one canonical documented command inventory.

- [ ] **Step 1: Extract motor code without behavior changes**

Move validation, command construction, pending ACK, and timeout tests into `motor.rs`; add logging only.

- [ ] **Step 2: Remove obsolete parallel architecture**

Keep only shared parser code required by `mavlink_monitor`; delete unused connection/session APIs that are not registered or documented.

- [ ] **Step 3: Update every backend document from the canonical manifest**

Include exact command names, full JSON, enum labels and numeric values, units, validation, retries/timeouts, disarm requirements, and manual-verification limitations. Correct the frame timeout text.

- [ ] **Step 4: Run documentation and endpoint parity tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib endpoint_manifest`

Expected: PASS and no manifest command missing from `lib.rs`.

- [ ] **Step 5: Commit and push**

Commit message: `Complete backend module and API documentation cleanup`

### Task 9: Final verification and release report

**Files:**
- Modify only if verification exposes a concrete defect.

**Interfaces:**
- Consumes: all previous task outputs.
- Produces: verified commit on `main` and a release-readiness report with hardware limitations.

- [ ] **Step 1: Run formatting and whitespace gates**

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`

Run: `git diff --check`

Expected: PASS.

- [ ] **Step 2: Run the complete library suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Expected: PASS; report Windows error 4551 as a policy block if it occurs rather than calling it a test failure.

- [ ] **Step 3: Check every Rust target**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`

Expected: PASS.

- [ ] **Step 4: Build the native application**

Run: `cargo build --manifest-path src-tauri/Cargo.toml --bin datum`

Expected: PASS. If Windows prevents execution after a successful build, report the policy issue separately.

- [ ] **Step 5: Verify frontend source is unchanged and Git is synchronized**

Compare frontend paths to the pre-plan commit, confirm only authorized backend/docs changes, then verify `HEAD == origin/main`.

- [ ] **Step 6: Commit any verification-only fixes and push**

Commit message if needed: `Finalize backend release readiness`

- [ ] **Step 7: Report verified scope**

List software tests, build status, log location, new discovery commands, compatibility notes, and the fact that only accelerometer and compass have completed physical hardware tests.

