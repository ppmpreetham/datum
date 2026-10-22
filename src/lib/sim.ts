import type { Backend } from "./backend";
import { attitude, nedEuler } from "./attitude";
import { SphereCoverage } from "./coverage";
import type {
  AccelPosition,
  AccelerometerStatus,
  AttitudeSample,
  CalibrationPhase,
  CalibrationProgress,
  CompassStatus,
  EscStatus,
  RadioChannel,
  RadioStatus,
  VehicleStatus,
} from "./types";

class Emitter<T> {
  private listeners = new Set<(value: T) => void>();
  on = (cb: (value: T) => void) => {
    this.listeners.add(cb);
    return () => void this.listeners.delete(cb);
  };
  emit(value: T) {
    this.listeners.forEach((cb) => cb(value));
  }
}

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const ACCEL_ORDER: AccelPosition[] = ["level", "left", "right", "nose_down", "nose_up", "upside_down"];

const ESC_STEPS = [
  ["remove_propellers", "Remove every propeller and secure the vehicle."],
  ["disconnect_battery", "Disconnect flight battery power."],
  ["raise_throttle", "Move transmitter throttle to maximum."],
  ["power_and_confirm_tones", "Power up and confirm the calibration tones."],
  ["lower_throttle", "Move throttle to minimum and confirm the completion tones."],
] as const;

/**
 * Simulated stick/switch inputs for a six-channel FlySky FS-i6 (AETR, mode 2).
 * Channels 7-8 exist on iBUS but stay centred, like a real FS-iA6B.
 */
export const simRadioInputs = [1500, 1500, 1000, 1500, 1000, 1000, 1500, 1500];

export function setSimChannel(channel: number, value: number) {
  simRadioInputs[channel - 1] = Math.round(Math.min(2000, Math.max(1000, value)));
}

/**
 * A pretend ArduPilot quad. Attitude comes from whatever drives the shared
 * attitude state in sim mode (mouse drag or a phone), so calibrations react to
 * what the user actually does.
 */
export class SimBackend implements Backend {
  readonly kind = "sim" as const;

  private vehicle = new Emitter<VehicleStatus>();
  private attitudeEvents = new Emitter<AttitudeSample>();
  private accel = new Emitter<AccelerometerStatus>();
  private compass = new Emitter<CompassStatus>();
  private radio = new Emitter<RadioStatus>();

  private state: VehicleStatus = {
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
  private parameters: Record<string, number> = {
    AHRS_ORIENTATION: 0,
    COMPASS_EXTERNAL: 1,
    COMPASS_ORIENT: 0,
    COMPASS_USE: 1,
    COMPASS_AUTO_ROT: 0,
    MOT_PWM_TYPE: 0,
  };
  private ticker: number | undefined;
  private generation = 0;

  private accelSession: AccelerometerStatus | null = null;
  private accelIndex = 0;
  private compassSession: CompassStatus | null = null;
  private coverage = new SphereCoverage();
  private stillFor = 0;
  private radioSession: RadioStatus = {
    session_id: null,
    status: "idle",
    signal_available: true,
    channels: [],
  };
  private escSession: EscStatus | null = null;
  private escIndex = 0;

  private update(patch: Partial<VehicleStatus>) {
    this.state = { ...this.state, ...patch };
    this.vehicle.emit(this.state);
  }

  private progress(calibration: CalibrationProgress) {
    this.update({ calibration });
  }

  listPorts = async () => [];
  connect = async (address: string) => {
    this.update({ phase: "connecting", address, message: "Waiting for a heartbeat" });
    await wait(700);
    this.update({
      phase: "connected",
      firmware: "ardu_pilot",
      system_id: 1,
      component_id: 1,
      armed: false,
      message: "Flight controller connected",
      sensors: [
        { kind: "gyro", condition: "healthy" },
        { kind: "accelerometer", condition: "healthy" },
        { kind: "compass", condition: "healthy" },
        { kind: "radio", condition: "healthy" },
      ],
    });
    this.startTicker();
    return this.state;
  };
  connectSerial = (portName: string, baudRate: number) =>
    this.connect(`serial:${portName}:${baudRate}`);
  disconnect = async () => {
    window.clearInterval(this.ticker);
    this.generation++;
    this.update({ phase: "disconnected", firmware: null, calibration: null, message: "No vehicle connected" });
    return this.state;
  };
  status = async () => this.state;

  onVehicle = this.vehicle.on;
  onAttitude = this.attitudeEvents.on;
  onAccelerometer = this.accel.on;
  onCompass = this.compass.on;
  onRadio = this.radio.on;

  readParameters = async (names: string[]) => {
    await wait(150);
    return Object.fromEntries(
      names.filter((name) => name in this.parameters).map((name) => [name, this.parameters[name]]),
    );
  };
  writeParameters = async (parameters: Record<string, number>) => {
    await wait(350);
    Object.assign(this.parameters, parameters);
    return parameters;
  };

  /** 20 Hz: radio telemetry plus the progress of sim calibrations. */
  private startTicker() {
    window.clearInterval(this.ticker);
    this.ticker = window.setInterval(() => this.tick(0.05), 50);
  }

  private tick(dt: number) {
    const calibration = this.state.calibration;
    const active = calibration && ["starting", "in_progress"].includes(calibration.phase);
    if (active && (calibration.kind === "gyro" || calibration.kind === "level")) {
      const still = attitude.angularSpeed < 0.25;
      this.stillFor = still ? this.stillFor + dt : 0;
      if (!still) {
        this.progress({ ...calibration, phase: "in_progress", message: "Motion detected — hold still" });
      } else if (this.stillFor > 0.3) {
        const percent = Math.min(100, (calibration.percent ?? 0) + dt * 30);
        const done = percent >= 100;
        // Like the real AHRS: once trimmed, the current attitude reads as level.
        if (done && calibration.kind === "level") {
          attitude.setEuler(0, 0, nedEuler(attitude.quat).yaw, attitude.source === "none" ? "drag" : attitude.source);
        }
        this.progress({
          ...calibration,
          phase: done ? "completed" : "in_progress",
          percent: Math.round(percent),
          message: done ? "Calibration successful" : "Sampling",
        });
      }
    }

    if (this.compassSession && this.compassSession.status === "in_progress") {
      this.coverage.update(attitude.quat);
      const percent = Math.min(100, Math.round((this.coverage.fraction() / 0.92) * 100));
      const done = percent >= 100;
      this.compassSession = {
        ...this.compassSession,
        status: done ? "completed" : "in_progress",
        instruction: done ? "Calibration successful — reboot the vehicle" : "Rotate the vehicle",
        sensors: [
          {
            compass_id: 0,
            progress_percent: percent,
            calibration_state: done ? "success" : "running_step_two",
            fitness: done ? 4.2 : null,
            fitness_source: done ? "mag_cal_report" : "not_available",
            saved: done,
            failure_reason: null,
          },
        ],
      };
      this.compass.emit(this.compassSession);
      this.progress({
        kind: "compass",
        phase: this.compassSession.status,
        percent,
        message: this.compassSession.instruction,
      });
    }

    const channels: RadioChannel[] = simRadioInputs.map((value, i) => {
      const existing = this.radioSession.channels[i];
      const capturing = this.radioSession.status === "capturing";
      const minimum = capturing && existing ? Math.min(existing.minimum_us, value) : value;
      const maximum = capturing && existing ? Math.max(existing.maximum_us, value) : value;
      return {
        channel: i + 1,
        value_us: value,
        minimum_us: minimum,
        maximum_us: maximum,
        trim_us: existing && capturing ? existing.trim_us : value,
        range_valid: maximum - minimum >= 400,
      };
    });
    this.radioSession = { ...this.radioSession, channels, signal_available: true };
    this.radio.emit(this.radioSession);
  }

  startCalibration = async (kind: "gyro" | "level" | "compass") => {
    if (kind === "compass") throw new Error("The simulator is ArduPilot; use onboard compass calibration.");
    this.requireIdle();
    this.stillFor = 0;
    this.progress({ kind, phase: "starting", percent: 0, message: "Preparing calibration request" });
  };
  stopCalibration = async () => {
    if (this.state.calibration) this.progress({ ...this.state.calibration, phase: "stopped" });
  };

  private requireIdle() {
    const phase = this.state.calibration?.phase;
    if (phase && ["starting", "in_progress", "awaiting_user"].includes(phase)) {
      throw new Error("Finish or stop the current calibration before starting another.");
    }
  }

  private emitAccel(patch: Partial<AccelerometerStatus>) {
    if (!this.accelSession) return;
    this.accelSession = { ...this.accelSession, ...patch };
    this.accel.emit(this.accelSession);
    this.progress({
      kind: "accelerometer",
      phase: this.accelSession.status,
      percent: this.accelSession.percent,
      message: this.accelSession.instruction,
    });
  }

  private async requestAccelPosition(generation: number) {
    await wait(700);
    if (generation !== this.generation || !this.accelSession) return;
    const orientation = ACCEL_ORDER[this.accelIndex];
    this.emitAccel({
      status: "awaiting_user",
      orientation,
      current_step: this.accelIndex + 1,
      waiting_for_confirmation: true,
      percent: Math.round((this.accelIndex / 6) * 100),
      instruction: `Place the drone ${orientation.replace("_", " ")}, then press Continue`,
    });
  }

  startAccelerometer = async () => {
    this.requireIdle();
    this.generation++;
    this.accelIndex = 0;
    this.accelSession = {
      session_id: `accel-sim-${Date.now()}`,
      status: "starting",
      current_step: 1,
      total_steps: 6,
      orientation: null,
      instruction: "Starting",
      waiting_for_confirmation: false,
      percent: 0,
    };
    this.emitAccel({});
    void this.requestAccelPosition(this.generation);
    return this.accelSession;
  };

  confirmAccelerometerPosition = async (sessionId: string, step: AccelPosition) => {
    const session = this.accelSession;
    if (!session || session.session_id !== sessionId) throw new Error("INVALID_SESSION");
    if (session.orientation !== step) throw new Error(`POSITION_MISMATCH: expected ${session.orientation}`);
    this.emitAccel({ status: "in_progress", waiting_for_confirmation: false, instruction: "Sampling" });
    const generation = this.generation;
    void (async () => {
      await wait(900);
      if (generation !== this.generation) return;
      this.accelIndex++;
      if (this.accelIndex >= 6) {
        this.emitAccel({
          status: "completed",
          orientation: null,
          percent: 100,
          instruction: "Calibration successful",
        });
      } else {
        void this.requestAccelPosition(generation);
      }
    })();
    return this.accelSession!;
  };

  cancelAccelerometer = async (sessionId: string) => {
    if (this.accelSession?.session_id !== sessionId) throw new Error("INVALID_SESSION");
    this.generation++;
    this.emitAccel({ status: "stopped", waiting_for_confirmation: false, instruction: "Cancelled" });
    return this.accelSession!;
  };

  compassSensors = async () => [
    {
      compass_id: 0,
      progress_percent: 0,
      calibration_state: "detected",
      fitness: null,
      fitness_source: "not_available",
      saved: false,
      failure_reason: null,
    },
  ];

  startCompass = async () => {
    this.requireIdle();
    this.coverage.reset();
    this.compassSession = {
      session_id: `compass-sim-${Date.now()}`,
      status: "in_progress" as CalibrationPhase,
      instruction: "Rotate the vehicle",
      sensors: [],
    };
    this.compass.emit(this.compassSession);
    return this.compassSession;
  };
  private compassAction(sessionId: string, status: CalibrationPhase) {
    if (this.compassSession?.session_id !== sessionId) throw new Error("INVALID_SESSION");
    this.compassSession = { ...this.compassSession, status };
    this.compass.emit(this.compassSession);
    this.progress({ kind: "compass", phase: status, percent: null, message: status });
    return this.compassSession;
  }
  acceptCompass = async (sessionId: string) => this.compassAction(sessionId, "completed");
  cancelCompass = async (sessionId: string) => this.compassAction(sessionId, "stopped");
  retryCompass = async (sessionId: string) => {
    this.coverage.reset();
    return this.compassAction(sessionId, "in_progress");
  };

  radioChannels = async () => this.radioSession;
  startRadio = async () => {
    this.radioSession = {
      ...this.radioSession,
      session_id: `radio-sim-${Date.now()}`,
      status: "capturing",
      channels: this.radioSession.channels.map((c) => ({
        ...c,
        minimum_us: c.value_us,
        maximum_us: c.value_us,
        trim_us: c.value_us,
        range_valid: false,
      })),
    };
    return this.radioSession;
  };
  finishRadio = async (sessionId: string) => {
    if (this.radioSession.session_id !== sessionId) throw new Error("INVALID_SESSION");
    const partial = this.radioSession.channels.filter((c) => {
      const travel = c.maximum_us - c.minimum_us;
      return travel >= 100 && travel < 400;
    });
    if (partial.length) {
      throw new Error(
        `INVALID_RC_RANGE: move channels ${partial.map((c) => c.channel).join(", ")} through their complete range before finishing.`,
      );
    }
    await wait(800);
    this.radioSession = { ...this.radioSession, status: "saved_and_verified" };
    return this.radioSession;
  };
  cancelRadio = async () => {
    this.radioSession = { ...this.radioSession, status: "stopped" };
    return this.radioSession;
  };

  escCapabilities = async () => ({
    pwm_type_value: 0,
    protocol: "normal_pwm",
    endpoint_calibration_required: true,
    software_can_verify_completion: false,
  });
  private escStatus(): EscStatus {
    const index = Math.min(this.escIndex, ESC_STEPS.length - 1);
    return {
      session_id: this.escSession!.session_id,
      active: this.escIndex < ESC_STEPS.length,
      step_id: ESC_STEPS[index][0],
      instruction: ESC_STEPS[index][1],
      manually_confirmed: this.escIndex >= ESC_STEPS.length,
    };
  }
  startEsc = async () => {
    this.escIndex = 0;
    this.escSession = { session_id: `esc-sim-${Date.now()}` } as EscStatus;
    this.escSession = this.escStatus();
    return this.escSession;
  };
  advanceEsc = async (sessionId: string, stepId: string) => {
    if (this.escSession?.session_id !== sessionId) throw new Error("INVALID_SESSION");
    if (ESC_STEPS[this.escIndex]?.[0] !== stepId) throw new Error("INVALID_STEP");
    this.escIndex++;
    this.escSession = this.escStatus();
    return this.escSession;
  };
  cancelEsc = async (sessionId: string) => {
    if (this.escSession?.session_id !== sessionId) throw new Error("INVALID_SESSION");
    this.escIndex = ESC_STEPS.length;
    this.escSession = { ...this.escStatus(), active: false, manually_confirmed: false };
    return this.escSession;
  };
  motorTest = async (motorNumber: number, throttlePercent: number, durationSeconds: number) => {
    await wait(250);
    return {
      accepted: true,
      motor_number: motorNumber,
      throttle_percent: throttlePercent,
      duration_seconds: durationSeconds,
      message: "Simulated motor test accepted",
    };
  };
}
