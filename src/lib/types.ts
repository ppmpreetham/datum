// Mirrors the serde shapes in src-tauri/src/calibration.rs.

export type Firmware = "ardu_pilot" | "px4";
export type Phase = "disconnected" | "connecting" | "connected" | "unsupported" | "error";
export type CalibrationKind =
  | "gyro"
  | "accelerometer"
  | "compass"
  | "level"
  | "radio"
  | "esc"
  | "motor_test";
export type CalibrationPhase =
  | "starting"
  | "accepted"
  | "in_progress"
  | "awaiting_user"
  | "stopping"
  | "stopped"
  | "completed"
  | "failed"
  | "unknown";

export type CalibrationProgress = {
  kind: CalibrationKind;
  phase: CalibrationPhase;
  percent: number | null;
  message: string;
};

export type VehicleStatus = {
  phase: Phase;
  address: string | null;
  firmware: Firmware | null;
  system_id: number | null;
  component_id: number | null;
  armed: boolean | null;
  message: string;
  calibration: CalibrationProgress | null;
  sensors: { kind: CalibrationKind; condition: "unknown" | "healthy" | "needs_attention" }[];
};

export const disconnectedStatus: VehicleStatus = {
  phase: "disconnected",
  address: null,
  firmware: null,
  system_id: null,
  component_id: null,
  armed: null,
  message: "No vehicle connected",
  calibration: null,
  sensors: [],
};

export type AttitudeSample = {
  roll: number;
  pitch: number;
  yaw: number;
  roll_speed: number;
  pitch_speed: number;
  yaw_speed: number;
  time_boot_ms: number;
};

export type SerialPortSummary = {
  portName: string;
  portType: string;
  usbVendorId: number | null;
  usbProductId: number | null;
  manufacturer: string | null;
  product: string | null;
  serialNumber: string | null;
};

export type AccelPosition = "level" | "left" | "right" | "nose_down" | "nose_up" | "upside_down";

export type AccelerometerStatus = {
  session_id: string;
  status: CalibrationPhase;
  current_step: number;
  total_steps: number;
  orientation: AccelPosition | null;
  instruction: string;
  waiting_for_confirmation: boolean;
  percent: number | null;
};

export type CompassSensor = {
  compass_id: number;
  progress_percent: number;
  calibration_state: string;
  fitness: number | null;
  fitness_source: string;
  saved: boolean;
  failure_reason: string | null;
};

export type CompassStatus = {
  session_id: string;
  status: CalibrationPhase;
  instruction: string;
  sensors: CompassSensor[];
};

export type RadioChannel = {
  channel: number;
  value_us: number;
  minimum_us: number;
  maximum_us: number;
  trim_us: number;
  range_valid: boolean;
};

export type RadioStatus = {
  session_id: string | null;
  status: string;
  signal_available: boolean;
  channels: RadioChannel[];
};

export type EscCapabilities = {
  pwm_type_value: number;
  protocol: string;
  endpoint_calibration_required: boolean;
  software_can_verify_completion: boolean;
};

export type EscStatus = {
  session_id: string;
  active: boolean;
  step_id: string;
  instruction: string;
  manually_confirmed: boolean;
};

export type MotorTestResponse = {
  accepted: boolean;
  motor_number: number;
  throttle_percent: number;
  duration_seconds: number;
  message: string;
};
