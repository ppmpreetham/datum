# Datum

An interactive Drone calibrator for your QGroundControl, inspired by astros's playroom

## Download these:

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

### Run:

```bash
pnpm tauri dev
```

On Windows, install the Rust and Visual Studio C++ prerequisites first. For
the Docker-assisted frontend workflow, see [SETUP.md](SETUP.md). It keeps the
Tauri desktop app on Windows and runs Vite in Docker.

## Calibration backend

Datum uses a Rust MAVLink backend inside Tauri. It can connect to one PX4 or
ArduPilot controller through USB serial, UDP, or TCP and exposes guided
calibration requests for gyro, accelerometer, compass, level, and radio. Motor
and ESC operations remain disabled pending hardware validation. A controller
acknowledgement is not treated as proof that a physical sensor is calibrated.

The React frontend handoff, API contract, and testing notes are in
[FRONTEND.md](FRONTEND.md). Windows and WSL2 setup are in
[SETUP.md](SETUP.md). ArduPilot SITL command behavior was checked; full
accelerometer calibration, PX4, and real drone validation remain pending.
