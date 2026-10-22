# Datum frontend integration

This document is the contract between the Rust/Tauri backend and the frontend. The backend owns the serial port, identifies the flight stack from a MAVLink heartbeat, sends calibration commands, and translates MAVLink progress into stable frontend data.

Mission Planner or QGroundControl cannot normally keep the same USB serial port open while Datum is connected. Close their vehicle connection before connecting Datum. Datum uses the same MAVLink calibration protocols; it does not call Mission Planner or QGroundControl as a service.

## Recommended frontend pipeline

1. Open `ws://127.0.0.1:8000/ws/calibration` before starting a calibration.
2. Call `list_serial_ports`, then either `connect_drone` with the selected port or `auto_connect_drone`.
3. Wait until `get_drone_status` returns `connectionPhase: "ready"`. Use `flightStack` to choose the correct magnetometer start payload.
4. Call `start_compass_calibration` once. Disable every other calibration button while its returned session is active.
5. Render progress and instructions from the calibration WebSocket. On first load or WebSocket reconnect, also call `get_current_calibration` and `get_calibration_progress` to restore state.
6. Stop on `completed`, `failed`, or `cancelled`. Show the backend's `instruction` and `error` text rather than guessing success from a progress value.
7. After successful compass calibration, tell the user to reboot the flight controller before flight unless the controller already rebooted.

All control calls use Tauri `invoke`; there is no HTTP REST API.

```ts
import { invoke } from "@tauri-apps/api/core";

type FlightStack = "arduPilot" | "px4" | "generic" | "unknown";

type DroneStatus = {
  connected: boolean;
  connectionPhase: "disconnected" | "connecting" | "ready" | "heartbeatLost" | "error";
  portName: string | null;
  baudRate: number | null;
  flightStack: FlightStack | null;
  systemId: number | null;
  componentId: number | null;
  lastError: string | null;
};

const ports = await invoke("list_serial_ports");
await invoke<DroneStatus>("connect_drone", {
  portName: "/dev/ttyACM0", // use COM3-style names on Windows
  baudRate: 115200,
});

const status = await invoke<DroneStatus>("get_drone_status");
if (status.connectionPhase !== "ready") throw new Error("Flight controller is not ready");
```

## Magnetometer start payload

For ArduPilot, pass an `options` object. These defaults match QGroundControl's normal onboard compass flow: calibrate all enabled compasses, do not retry automatically, save successful results, and let the user reboot deliberately.

```ts
const started = await invoke("start_compass_calibration", {
  options: {
    compassMask: 0,       // 0 = all enabled compasses; otherwise use an ArduPilot bitmask
    retryOnFailure: false,
    autosave: true,
    autoReboot: false,
  },
});
```

For PX4, omit `options`. PX4 uses `MAV_CMD_PREFLIGHT_CALIBRATION` with the magnetometer action set to start.

```ts
const started = await invoke("start_compass_calibration");
```

The start response has this shape:

```ts
type CalibrationStart = {
  calibration: "compass";
  flightStack: "arduPilot" | "px4";
  commandSent: {
    command: number;
    targetSystem: number;
    targetComponent: number;
    sentAtMs: number;
  };
  nextStep: string;
};
```

## Live calibration events

The calibration WebSocket is server-to-client only. Each message is JSON:

```ts
type CalibrationEventMessage = {
  type: "calibrationEvent";
  payload: {
    eventType:
      | "commandAck"
      | "statusText"
      | "accelerometerPosition"
      | "magCalProgress"
      | "magCalReport";
    message: string;
    severity: number | null;
    command: number | null;
    result: number | null;
    progress: number | null;
    receivedAtMs: number;
  };
};

const socket = new WebSocket("ws://127.0.0.1:8000/ws/calibration");
socket.onmessage = async ({ data }) => {
  const event = JSON.parse(data) as CalibrationEventMessage;
  if (event.type !== "calibrationEvent") return;

  renderMessage(event.payload.message);
  if (event.payload.progress !== null) renderProgress(event.payload.progress);

  // The session is authoritative for terminal state and errors.
  const session = await invoke("get_current_calibration");
  renderCalibrationSession(session);
};
```

Do not treat `100%` alone as success. For ArduPilot, the backend evaluates `MAG_CAL_REPORT`; for PX4, it evaluates command acknowledgements and calibration status text. A connected client receives only new events, so hydrate state with:

```ts
const [session, history] = await Promise.all([
  invoke("get_current_calibration"),
  invoke("get_calibration_progress"),
]);
```

The retained history contains at most 200 events.

## Cancel and accept

ArduPilot exposes structured magnetometer cancel and accept commands:

```ts
await invoke("cancel_compass_calibration");
await invoke("accept_compass_calibration");
```

`accept_compass_calibration` is mainly useful when `autosave` was false. PX4 does not expose these two backend calls because its standard calibration flow uses its preflight-calibration protocol and status prompts.

## User interface requirements

- Require `connectionPhase === "ready"` before enabling Start.
- Show a clear disarmed warning and instruct the user to remove propellers.
- Tell the user to move away from computers, power supplies, steel tables, magnets, and high-current wiring.
- Display `nextStep`, live `message`, progress, `compassMask`, each compass result (`successful`, `fitness`, and `autosaved`), and the terminal `error` exactly as returned.
- Keep Cancel visible during an ArduPilot compass calibration.
- If the heartbeat is lost, freeze the progress UI and show `get_drone_status().lastError`.
- Do not fabricate progress while waiting for the first MAVLink progress report.

The full machine-readable command contract is in `endpoints.json` and can also be loaded at runtime with `get_backend_manifest`.

## Protocol references

- MAVLink common commands: <https://mavlink.io/en/messages/common.html#MAV_CMD_PREFLIGHT_CALIBRATION>
- ArduPilot MAVLink calibration protocol: <https://ardupilot.org/dev/docs/mavlink-calibration.html>
- ArduPilot compass calibration guidance: <https://ardupilot.org/copter/docs/common-compass-calibration-in-mission-planner.html>
- PX4 compass calibration guidance: <https://docs.px4.io/main/en/config/compass>
- QGroundControl ArduPilot sensor controller: <https://github.com/mavlink/qgroundcontrol/blob/master/src/AutoPilotPlugins/APM/APMSensorsComponentController.cc>
