import * as THREE from "three";
import { EARTH_FIELD_NED } from "./attitude";

const CELL_COUNT = 110;
const PAINT_RADIUS = THREE.MathUtils.degToRad(21);
const FULL_TURN = Math.PI * 2;

function fibonacciSphere(count: number) {
  const golden = Math.PI * (3 - Math.sqrt(5));
  return Array.from({ length: count }, (_, i) => {
    const y = 1 - (i / (count - 1)) * 2;
    const r = Math.sqrt(1 - y * y);
    const theta = golden * i;
    return new THREE.Vector3(Math.cos(theta) * r, y, Math.sin(theta) * r);
  });
}

export type SpinAxis = "roll" | "pitch" | "yaw";

export type CoverageHint = {
  /** Unpainted cell to bring under the brush (body frame, NED). */
  target: number;
  /** Rotation axis in NED world frame; turning about it (right-handed) helps. */
  axisWorld: THREE.Vector3;
  /** Which body axis that turn mostly is, and in which sense. */
  spin: SpinAxis;
  sign: 1 | -1;
};

/**
 * Tracks which directions of Earth's magnetic field the drone's body has
 * "seen", exactly what an onboard compass calibration needs. Visually it's a
 * sphere that rolls under a fixed brush (the field direction).
 */
export class SphereCoverage {
  readonly cells = fibonacciSphere(CELL_COUNT);
  /** performance.now() when each cell was painted, or 0. */
  readonly paintedAt = new Float64Array(CELL_COUNT);
  /** Accumulated rotation about each body axis (radians, capped at 2π). */
  readonly spin = { roll: 0, pitch: 0, yaw: 0 };
  private readonly previous = new THREE.Quaternion();
  private hasPrevious = false;
  private painted = 0;
  private readonly brushBody = new THREE.Vector3();
  private readonly cosRadius = Math.cos(PAINT_RADIUS);

  reset() {
    this.paintedAt.fill(0);
    this.painted = 0;
    this.spin.roll = this.spin.pitch = this.spin.yaw = 0;
    this.hasPrevious = false;
  }

  /** Field direction in body frame for a body→NED attitude. */
  brush(quat: THREE.Quaternion, out = this.brushBody) {
    return out.copy(EARTH_FIELD_NED).applyQuaternion(quat.clone().invert());
  }

  update(quat: THREE.Quaternion, now = performance.now()) {
    const brush = this.brush(quat);
    const fresh: number[] = [];
    this.cells.forEach((cell, i) => {
      if (this.paintedAt[i] === 0 && cell.dot(brush) > this.cosRadius) {
        this.paintedAt[i] = now;
        this.painted++;
        fresh.push(i);
      }
    });

    if (this.hasPrevious) {
      // Body-frame increment: prev⁻¹ · current.
      const delta = this.previous.clone().invert().multiply(quat);
      if (delta.w < 0) delta.set(-delta.x, -delta.y, -delta.z, -delta.w);
      const angle = 2 * Math.acos(Math.min(1, delta.w));
      if (angle > 1e-4 && angle < 1) {
        const s = Math.sqrt(1 - delta.w * delta.w) || 1;
        this.spin.roll = Math.min(FULL_TURN, this.spin.roll + Math.abs((delta.x / s) * angle));
        this.spin.pitch = Math.min(FULL_TURN, this.spin.pitch + Math.abs((delta.y / s) * angle));
        this.spin.yaw = Math.min(FULL_TURN, this.spin.yaw + Math.abs((delta.z / s) * angle));
      }
    }
    this.previous.copy(quat);
    this.hasPrevious = true;
    return fresh;
  }

  fraction() {
    return this.painted / CELL_COUNT;
  }

  /** Suggest the turn that paints the biggest nearby unpainted patch. */
  hint(quat: THREE.Quaternion): CoverageHint | null {
    if (this.painted >= CELL_COUNT) return null;
    const brush = this.brush(quat, new THREE.Vector3());
    let best = -1;
    let bestScore = -Infinity;
    const neighbourCos = Math.cos(THREE.MathUtils.degToRad(45));
    this.cells.forEach((cell, i) => {
      if (this.paintedAt[i] !== 0) return;
      let patch = 0;
      this.cells.forEach((other, j) => {
        if (this.paintedAt[j] === 0 && cell.dot(other) > neighbourCos) patch++;
      });
      // Prefer big patches, but not ones on the far side of the sphere.
      const score = patch + cell.dot(brush) * 4;
      if (score > bestScore) {
        bestScore = score;
        best = i;
      }
    });
    if (best < 0) return null;

    const targetWorld = this.cells[best].clone().applyQuaternion(quat);
    let axisWorld = targetWorld.clone().cross(EARTH_FIELD_NED);
    if (axisWorld.lengthSq() < 1e-6) {
      axisWorld = new THREE.Vector3(0, 0, 1).cross(EARTH_FIELD_NED);
    }
    axisWorld.normalize();
    const axisBody = axisWorld.clone().applyQuaternion(quat.clone().invert());
    const components: [SpinAxis, number][] = [
      ["roll", axisBody.x],
      ["pitch", axisBody.y],
      ["yaw", axisBody.z],
    ];
    const [spin, value] = components.reduce((a, b) => (Math.abs(b[1]) > Math.abs(a[1]) ? b : a));
    return { target: best, axisWorld, spin, sign: value >= 0 ? 1 : -1 };
  }
}
