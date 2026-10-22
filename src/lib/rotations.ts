import * as THREE from "three";
import type { Firmware } from "./types";

/**
 * Board rotation enum shared (values 0-38) by ArduPilot `AHRS_ORIENTATION` /
 * `COMPASS_ORIENT` and PX4 `SENS_BOARD_ROT` / `CAL_MAG0_ROT`. Names encode the
 * rotation applied roll first, then pitch, then yaw (R = Rz·Ry·Rx, NED).
 */
const SHARED = [
  "None",
  "Yaw45",
  "Yaw90",
  "Yaw135",
  "Yaw180",
  "Yaw225",
  "Yaw270",
  "Yaw315",
  "Roll180",
  "Roll180Yaw45",
  "Roll180Yaw90",
  "Roll180Yaw135",
  "Pitch180",
  "Roll180Yaw225",
  "Roll180Yaw270",
  "Roll180Yaw315",
  "Roll90",
  "Roll90Yaw45",
  "Roll90Yaw90",
  "Roll90Yaw135",
  "Roll270",
  "Roll270Yaw45",
  "Roll270Yaw90",
  "Roll270Yaw135",
  "Pitch90",
  "Pitch270",
  "Pitch180Yaw90",
  "Pitch180Yaw270",
  "Roll90Pitch90",
  "Roll180Pitch90",
  "Roll270Pitch90",
  "Roll90Pitch180",
  "Roll270Pitch180",
  "Roll90Pitch270",
  "Roll180Pitch270",
  "Roll270Pitch270",
  "Roll90Pitch180Yaw90",
  "Roll90Yaw270",
  "Roll90Pitch68Yaw293",
];

const ARDUPILOT_EXTRA: [number, string][] = [
  [39, "Pitch7"],
  [40, "Roll45"],
  [41, "Roll315"],
];

const PX4_EXTRA: [number, string][] = [
  [39, "Pitch315"],
  [40, "Roll90Pitch315"],
];

export type RotationOption = {
  value: number;
  name: string;
  label: string;
  roll: number;
  pitch: number;
  yaw: number;
};

function parse(value: number, name: string): RotationOption {
  const angles = { roll: 0, pitch: 0, yaw: 0 };
  for (const [, axis, degrees] of name.matchAll(/(Roll|Pitch|Yaw)(\d+)/g)) {
    angles[axis.toLowerCase() as keyof typeof angles] = Number(degrees);
  }
  const parts = (["roll", "pitch", "yaw"] as const)
    .filter((axis) => angles[axis] !== 0)
    .map((axis) => `${axis[0].toUpperCase()}${axis.slice(1)} ${angles[axis]}°`);
  return { value, name, label: parts.length ? parts.join(" · ") : "None", ...angles };
}

export function rotationOptions(firmware: Firmware | null): RotationOption[] {
  const extra = firmware === "px4" ? PX4_EXTRA : ARDUPILOT_EXTRA;
  return [...SHARED.map((name, value) => [value, name] as [number, string]), ...extra].map(
    ([value, name]) => parse(value, name),
  );
}

export function rotationParameter(firmware: Firmware | null, target: "board" | "compass") {
  if (firmware === "px4") return target === "board" ? "SENS_BOARD_ROT" : "CAL_MAG0_ROT";
  return target === "board" ? "AHRS_ORIENTATION" : "COMPASS_ORIENT";
}

/** Rotation as a quaternion in the NED basis. */
export function rotationQuatNed(option: RotationOption) {
  const d = THREE.MathUtils.degToRad;
  return new THREE.Quaternion().setFromEuler(
    new THREE.Euler(d(option.roll), d(option.pitch), d(option.yaw), "ZYX"),
  );
}
