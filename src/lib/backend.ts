import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AccelPosition,
  AccelerometerStatus,
  AttitudeSample,
  CompassSensor,
  CompassStatus,
  EscCapabilities,
  EscStatus,
  MotorTestResponse,
  RadioStatus,
  SerialPortSummary,
  VehicleStatus,
} from "./types";

type Unlisten = () => void;

/**
 * Everything the guided flows need from a flight controller. The Tauri
 * implementation talks to the Rust MAVLink backend; the simulator implements
 * the same contract so every flow can be rehearsed without hardware.
 */
export interface Backend {
  readonly kind: "live" | "sim";
  listPorts(): Promise<SerialPortSummary[]>;
  connect(address: string): Promise<VehicleStatus>;
  connectSerial(portName: string, baudRate: number): Promise<VehicleStatus>;
  disconnect(): Promise<VehicleStatus>;
  status(): Promise<VehicleStatus>;

  onVehicle(cb: (status: VehicleStatus) => void): Unlisten;
  onAttitude(cb: (sample: AttitudeSample) => void): Unlisten;
  onAccelerometer(cb: (status: AccelerometerStatus) => void): Unlisten;
  onCompass(cb: (status: CompassStatus) => void): Unlisten;
  onRadio(cb: (status: RadioStatus) => void): Unlisten;

  readParameters(names: string[]): Promise<Record<string, number>>;
  writeParameters(parameters: Record<string, number>): Promise<Record<string, number>>;

  /** Gyro, level (and PX4 compass) use the generic preflight-calibration command. */
  startCalibration(kind: "gyro" | "level" | "compass"): Promise<void>;
  stopCalibration(): Promise<void>;

  startAccelerometer(): Promise<AccelerometerStatus>;
  confirmAccelerometerPosition(sessionId: string, step: AccelPosition): Promise<AccelerometerStatus>;
  cancelAccelerometer(sessionId: string): Promise<AccelerometerStatus>;

  compassSensors(): Promise<CompassSensor[]>;
  startCompass(request: {
    compass_ids: number[];
    retry_on_failure: boolean;
    autosave: boolean;
  }): Promise<CompassStatus>;
  acceptCompass(sessionId: string): Promise<CompassStatus>;
  cancelCompass(sessionId: string): Promise<CompassStatus>;
  retryCompass(sessionId: string): Promise<CompassStatus>;

  radioChannels(): Promise<RadioStatus>;
  startRadio(): Promise<RadioStatus>;
  finishRadio(sessionId: string): Promise<RadioStatus>;
  cancelRadio(sessionId: string): Promise<RadioStatus>;

  escCapabilities(): Promise<EscCapabilities>;
  startEsc(): Promise<EscStatus>;
  advanceEsc(sessionId: string, stepId: string): Promise<EscStatus>;
  cancelEsc(sessionId: string): Promise<EscStatus>;
  motorTest(motorNumber: number, throttlePercent: number, durationSeconds: number): Promise<MotorTestResponse>;
}

export const isTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function subscribe<T>(event: string, cb: (payload: T) => void): Unlisten {
  let disposed = false;
  let unlisten: Unlisten | undefined;
  listen<T>(event, ({ payload }) => cb(payload)).then((fn) => {
    if (disposed) fn();
    else unlisten = fn;
  });
  return () => {
    disposed = true;
    unlisten?.();
  };
}

/** Tauri rejects with the Rust `Err(String)`; surface it as a normal Error. */
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw new Error(typeof error === "string" ? error : String(error));
  }
}

export class TauriBackend implements Backend {
  readonly kind = "live" as const;

  listPorts = () => call<SerialPortSummary[]>("list_serial_ports");
  connect = (address: string) => call<VehicleStatus>("connect_vehicle", { address });
  connectSerial = (portName: string, baudRate: number) =>
    call<VehicleStatus>("connect_serial_vehicle", { portName, baudRate });
  disconnect = () => call<VehicleStatus>("disconnect_vehicle");
  status = () => call<VehicleStatus>("vehicle_status");

  onVehicle = (cb: (status: VehicleStatus) => void) => subscribe("datum://vehicle", cb);
  onAttitude = (cb: (sample: AttitudeSample) => void) => subscribe("datum://attitude", cb);
  onAccelerometer = (cb: (status: AccelerometerStatus) => void) =>
    subscribe("datum://accelerometer", cb);
  onCompass = (cb: (status: CompassStatus) => void) => subscribe("datum://compass", cb);
  onRadio = (cb: (status: RadioStatus) => void) => subscribe("datum://radio", cb);

  readParameters = (names: string[]) =>
    call<Record<string, number>>("read_vehicle_parameters", { names });
  writeParameters = (parameters: Record<string, number>) =>
    call<Record<string, number>>("write_vehicle_parameters", { parameters });

  startCalibration = (kind: "gyro" | "level" | "compass") => call<void>("start_calibration", { kind });
  stopCalibration = () => call<void>("stop_calibration");

  startAccelerometer = () =>
    call<AccelerometerStatus>("start_accelerometer_calibration", { calibrationType: "full" });
  confirmAccelerometerPosition = (sessionId: string, step: AccelPosition) =>
    call<AccelerometerStatus>("accelerometer_calibration_step", {
      request: { session_id: sessionId, step, action: "confirm_position" },
    });
  cancelAccelerometer = (sessionId: string) =>
    call<AccelerometerStatus>("cancel_accelerometer_calibration", {
      request: { session_id: sessionId },
    });

  compassSensors = () => call<CompassSensor[]>("compass_sensors");
  startCompass = (request: { compass_ids: number[]; retry_on_failure: boolean; autosave: boolean }) =>
    call<CompassStatus>("start_compass_calibration", { request });
  acceptCompass = (sessionId: string) =>
    call<CompassStatus>("accept_compass_calibration", { request: { session_id: sessionId } });
  cancelCompass = (sessionId: string) =>
    call<CompassStatus>("cancel_compass_calibration", { request: { session_id: sessionId } });
  retryCompass = (sessionId: string) =>
    call<CompassStatus>("retry_compass_calibration", { request: { session_id: sessionId } });

  radioChannels = () => call<RadioStatus>("radio_channels");
  startRadio = () =>
    call<RadioStatus>("start_radio_calibration", {
      request: { capture_min_max: true, capture_trim: true },
    });
  finishRadio = (sessionId: string) =>
    call<RadioStatus>("finish_radio_calibration", { request: { session_id: sessionId } });
  cancelRadio = (sessionId: string) =>
    call<RadioStatus>("cancel_radio_calibration", { request: { session_id: sessionId } });

  escCapabilities = () => call<EscCapabilities>("esc_capabilities");
  startEsc = () =>
    call<EscStatus>("start_esc_calibration", { request: { propellers_removed: true } });
  advanceEsc = (sessionId: string, stepId: string) =>
    call<EscStatus>("advance_esc_calibration", {
      request: { session_id: sessionId, step_id: stepId, action: "confirm_completed" },
    });
  cancelEsc = (sessionId: string) =>
    call<EscStatus>("cancel_esc_calibration", { request: { session_id: sessionId } });
  motorTest = (motorNumber: number, throttlePercent: number, durationSeconds: number) =>
    call<MotorTestResponse>("run_motor_test", {
      request: {
        motor_number: motorNumber,
        throttle_percent: throttlePercent,
        duration_seconds: durationSeconds,
        propellers_removed: true,
      },
    });
}
