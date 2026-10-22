# Datum

An interactive Drone calibrator for your QGroundControl, inspired by astros's playroom

## Download these:

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

### Run:

```bash
pnpm tauri dev
```

### Try it without a drone

Open the app in a plain browser (`pnpm dev`), or press **Try without a drone** in the desktop app. A simulated ArduPilot quad runs every flow: drag the screen to tilt the drone (hold Shift to roll), drag the sticks on the 3D transmitter, and click its switches. In the desktop app, a phone on `/sender` can stand in for the drone.

Keys: **Enter/Space** = ✕, **Esc** = ○, **S** = □.

### QGroundControl

Datum talks MAVLink to the flight controller itself (USB serial, UDP or TCP), so QGroundControl doesn't need to be running. QGroundControl has no HTTP/WebSocket API, and its "MAVLink forwarding" only sends telemetry *out* (messages arriving on the forwarding link are never relayed to the vehicle), so it can't carry calibration commands. Only one app can own a USB port: close QGroundControl's connection before connecting Datum.

The guided flows mirror QGroundControl's: the Autopilot Rotation prompt before each sensor calibration, internal/external compass plus compass rotation, the six-pose accelerometer order requested by the autopilot, and the radio stick sequence.

## TODO:

| Component                 | Status |
| ------------------------- | ------ |
| gyroscope                 | 🟢     |
| magnetometer              | 🟡     |
| accelerometer             | 🟡     |
| Level Horizon             | 🟡     |
| ESC transmission (TX, RX) | 🟡     |

- [ ] Combine it with QGroundControl
- [ ] Work with the actual Drone Hardware
