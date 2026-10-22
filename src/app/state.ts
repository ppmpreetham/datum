import { useEffect } from "react";
import type { Backend } from "../lib/backend";
import { TauriBackend, isTauri } from "../lib/backend";
import { SimBackend } from "../lib/sim";
import { say } from "../lib/sfx";
import { createStore, createTunnel, useStore } from "../lib/store";
import { disconnectedStatus, type VehicleStatus } from "../lib/types";
import { palette } from "../theme";

export type StepId = "connect" | "gyro" | "accel" | "compass" | "level" | "radio" | "esc";

export type StepInfo = { id: StepId; title: string; color: string; glyph: string };

export const CALIBRATION_STEPS: StepInfo[] = [
  { id: "gyro", title: "Gyroscope", color: palette.triangle, glyph: "◎" },
  { id: "accel", title: "Accelerometer", color: palette.circle, glyph: "⬢" },
  { id: "compass", title: "Compass", color: palette.cross, glyph: "✦" },
  { id: "level", title: "Level Horizon", color: palette.gold, glyph: "◉" },
  { id: "radio", title: "Radio", color: palette.square, glyph: "✚" },
  { id: "esc", title: "ESC", color: palette.cyan, glyph: "⚡" },
];

export type Vec3 = [number, number, number];

export type DroneControls = {
  visible: boolean;
  /** live = follow the vehicle, level = sit flat, hidden props etc. */
  pose: "live" | "level";
  opacity: number;
  spinProps: boolean;
  propsAttached: boolean;
  /** Per-motor speed multipliers in AP test order A-D (front-right, clockwise). */
  motorSpeeds: [number, number, number, number];
  highlightMotor: number | null;
  /** Mouse drag rotates the drone (simulator only). */
  draggable: boolean;
};

const defaultDrone: DroneControls = {
  visible: true,
  pose: "live",
  opacity: 1,
  spinProps: false,
  propsAttached: true,
  motorSpeeds: [0, 0, 0, 0],
  highlightMotor: null,
  draggable: true,
};

type AppState = {
  backend: Backend;
  vehicle: VehicleStatus;
  step: StepId;
  completed: Partial<Record<StepId, boolean>>;
  caption: { text: string; tone: "info" | "good" | "warn"; id: number };
  camera: { position: Vec3; target: Vec3; fov: number };
  drone: DroneControls;
  muted: boolean;
  voice: boolean;
  error: { text: string; id: number } | null;
};

export const app = createStore<AppState>({
  backend: isTauri() ? new TauriBackend() : new SimBackend(),
  vehicle: disconnectedStatus,
  step: "connect",
  completed: {},
  caption: { text: "", tone: "info", id: 0 },
  camera: { position: [0, 0.9, 3.4], target: [0, 0, 0], fov: 40 },
  drone: defaultDrone,
  muted: false,
  voice: true,
  error: null,
});

export const useApp = <U>(selector: (state: AppState) => U) => useStore(app, selector);

/** 3D content rendered by DOM-side step components. */
export const SceneTunnel = createTunnel();

let captionId = 0;
/** Show a short line in the speech bubble, and say it out loud. */
export function speak(text: string, tone: AppState["caption"]["tone"] = "info", voice = true) {
  const current = app.get().caption;
  if (current.text === text) return;
  app.set({ caption: { text, tone, id: ++captionId } });
  if (voice) say(text);
}

let errorId = 0;
export function showError(error: unknown) {
  const text = error instanceof Error ? error.message : String(error);
  app.set({ error: { text: humanizeError(text), id: ++errorId } });
}

/** Backend errors carry SCREAMING_CODES; keep the human part. */
function humanizeError(text: string) {
  return text.replace(/^[A-Z_]+:\s*/, "");
}

export function setCamera(position: Vec3, target: Vec3, fov = 40) {
  const { camera } = app.get();
  if (
    camera.position.join() === position.join() &&
    camera.target.join() === target.join() &&
    camera.fov === fov
  ) {
    return;
  }
  app.set({ camera: { position, target, fov } });
}

/**
 * Point the camera while mounted. Parent steps pass `enabled = false` while a
 * child (e.g. the rotation sheet) owns the shot, since parent effects run last.
 */
export function useCamera(position: Vec3, target: Vec3, fov = 40, enabled = true) {
  const key = [...position, ...target, fov, enabled].join();
  useEffect(() => {
    if (enabled) setCamera(position, target, fov);
  }, [key]);
}

export function setDrone(patch: Partial<DroneControls>) {
  app.set(({ drone }) => ({ drone: { ...drone, ...patch } }));
}

/** Apply drone settings while a step (or phase) is mounted. */
export function useDrone(patch: Partial<DroneControls>) {
  const key = JSON.stringify(patch);
  useEffect(() => {
    setDrone({ ...defaultDrone, ...patch });
  }, [key]);
}

export function goTo(step: StepId) {
  app.set({ step, error: null });
}

export function markComplete(step: StepId) {
  app.set(({ completed }) => ({ completed: { ...completed, [step]: true } }));
}

export function nextStep(after: StepId) {
  const index = CALIBRATION_STEPS.findIndex((s) => s.id === after);
  const next = CALIBRATION_STEPS[index + 1];
  goTo(next ? next.id : after);
}

export function useBackend() {
  return useApp((s) => s.backend);
}

export function useVehicle() {
  return useApp((s) => s.vehicle);
}
