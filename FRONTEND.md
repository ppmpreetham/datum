# Datum frontend handoff

Build the user interface in the existing React/Tauri app. The Rust backend lives in
`src-tauri/src/calibration.rs`. Use Tauri `invoke` for requests and `listen` for
updates. The legacy `/ws/gyro` relay is unrelated to vehicle calibration.

## User flow

1. Show a connection selector: USB serial (`serial:COM3:115200`), local UDP
   (`udpin:127.0.0.1:14550`), UDP client (`udpout:127.0.0.1:14550`), or TCP
   client (`tcpout:127.0.0.1:5760`). Allow one vehicle at a time.
2. Subscribe to `datum://vehicle` before connecting. Call `vehicle_status`
   after subscribing to fetch the current snapshot.
3. Show `connecting` until the first autopilot heartbeat. Display firmware,
   system ID, arm state, connection errors, and `message` prominently.
4. Call `calibration_options` to build the component chooser. Display each
   option's instructions and note. Disabled options have `available: false`.
5. When connected and disarmed, let the user choose one component and press
   Start. Call `start_calibration` with its `kind`. Show every phase and message.
6. For `awaiting_user`, show the controller's requested orientation and a
   Continue button. Call `continue_calibration` only after the user confirms
   the drone has been placed as instructed.
7. Keep a Stop button visible during `starting`, `accepted`, `in_progress`,
   `awaiting_user`, and `stopping`. Call `stop_calibration`. Stop is confirmed only
   when the phase becomes `stopped`. If it becomes `unknown`, tell the user to
   check the flight controller before retrying.
8. Show `sensors` as controller-reported health hints. `healthy` does **not**
   mean calibrated. `needs_attention` means the controller reports an unhealthy
   sensor, which may require inspection as well as calibration. `unknown`
   means no useful status was reported.
9. Allow Disconnect and a new connection only after the previous one ends.

## Tauri API

```ts
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

type Firmware = 'ardu_pilot' | 'px4';
type Kind = 'gyro' | 'accelerometer' | 'compass' | 'level' | 'radio' | 'esc' | 'motor_test';
type Phase = 'disconnected' | 'connecting' | 'connected' | 'unsupported' | 'error';
type CalibrationPhase = 'starting' | 'accepted' | 'in_progress' |
  'awaiting_user' | 'stopping' | 'stopped' | 'completed' | 'failed' | 'unknown';

interface VehicleStatus {
  phase: Phase;
  address: string | null;
  firmware: Firmware | null;
  system_id: number | null;
  component_id: number | null;
  armed: boolean | null;
  message: string;
  calibration: null | {
    kind: Kind;
    phase: CalibrationPhase;
    percent: number | null;
    message: string;
  };
  sensors: { kind: Kind; condition: 'unknown' | 'healthy' | 'needs_attention' }[];
}

interface CalibrationOption {
  kind: Kind;
  available: boolean;
  instructions: string;
  note: string;
}

const unlisten = await listen<VehicleStatus>('datum://vehicle', event => {
  // Replace your current snapshot with event.payload.
});
const status = await invoke<VehicleStatus>('vehicle_status');
const options = await invoke<CalibrationOption[]>('calibration_options');
await invoke<VehicleStatus>('connect_vehicle', { address: 'udpin:127.0.0.1:14550' });
await invoke<void>('start_calibration', { kind: 'gyro' });
await invoke<void>('continue_calibration'); // Only when phase is awaiting_user.
await invoke<void>('stop_calibration');
await invoke<VehicleStatus>('disconnect_vehicle');
// Call unlisten() when this view unmounts.
```

`invoke` rejects with a human-readable error string for invalid addresses,
unsupported firmware, an armed vehicle, a second connection, an unavailable
motor operation, or a lost connection. Show that error beside the attempted
action. Do not turn `accepted` into a success screen: MAVLink acceptance does
not prove calibration completed. `unknown` also never means success.

## What to design

Use plain language. The main screen should show connection status and a
component list with health hints. A selected component should have a short
preparation step, Start, live controller instructions, progress when available,
and Stop. Show controller messages verbatim in a secondary activity area.
Keep simulator and real hardware connections visibly distinct in the
connection chooser. Do not expose arm, motor, or ESC actions.

## Running and testing

From the repo root on Windows:

```powershell
pnpm install
pnpm tauri dev
```

Rust and Visual Studio C++ Build Tools are required for Tauri. For SITL, run
the firmware simulator in WSL2, forward its MAVLink connection to Windows,
then connect Datum using the corresponding address. See the official
[ArduPilot WSL SITL guide](https://ardupilot.org/dev/docs/sitl-on-windows-wsl.html)
and [PX4 WSL2 guide](https://docs.px4.io/main/en/dev_setup/dev_env_windows_wsl.html).
Confirm the actual UDP/TCP endpoint in the simulator output; ports differ by
configuration. A local UDP listener only binds to `127.0.0.1` in this backend.

## Backend limits to reflect in the UI

- Firmware is identified from the flight controller heartbeat; only PX4 and
  ArduPilot are accepted.
- Commands for gyro, accelerometer, compass, level, and PX4 radio calibration
  are sent through MAVLink. ArduPilot radio endpoint calibration requires its
  separate setup procedure; the backend rejects that request. Actual support
  varies with firmware version and vehicle. A rejected command is shown as
  failure.
- ESC calibration and motor testing are disabled until real hardware testing.
- Some firmware sends only command acceptance, not a final result. After one
  minute without completion, the result becomes `unknown`.
- Controller sensor health is not proof that a sensor has been calibrated.
- ArduPilot SITL accepted gyro and level requests, prompted for the first
  accelerometer position, and completed compass calibration with autosaved
  reports. The six-position accelerometer sequence, PX4, and physical
  calibration remain unverified.
- The frontend production build passes, but the current UI is not yet wired
  to these calibration commands and events. Implement the flow above before
  treating the interface as a complete calibration helper.
