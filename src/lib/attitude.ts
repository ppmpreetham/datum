import * as THREE from "three";

/*
 * Frames
 * ------
 * MAVLink attitude is NED: x forward, y right, z down.
 * The scene uses three.js axes with the drone's nose pointing away from the
 * camera: x right, y up, z toward the viewer. So a NED vector (x, y, z) maps
 * to three (y, -z, -x).
 */
const NED_TO_THREE = new THREE.Matrix4().set(0, 1, 0, 0, 0, 0, -1, 0, -1, 0, 0, 0, 0, 0, 0, 1);
const qC = new THREE.Quaternion().setFromRotationMatrix(NED_TO_THREE);
const qCi = qC.clone().invert();

export function nedToThree(v: THREE.Vector3, out = new THREE.Vector3()) {
  return out.set(v.y, -v.z, -v.x);
}

export function threeToNed(v: THREE.Vector3, out = new THREE.Vector3()) {
  return out.set(-v.z, v.x, -v.y);
}

/** Aerospace yaw-pitch-roll (R = Rz·Ry·Rx) as a quaternion in the NED basis. */
export function nedQuat(roll: number, pitch: number, yaw: number, out = new THREE.Quaternion()) {
  return out.setFromEuler(new THREE.Euler(roll, pitch, yaw, "ZYX"));
}

export function nedQuatToThree(q: THREE.Quaternion, out = new THREE.Quaternion()) {
  return out.copy(qC).multiply(q).multiply(qCi);
}

export function threeQuatToNed(q: THREE.Quaternion, out = new THREE.Quaternion()) {
  return out.copy(qCi).multiply(q).multiply(qC);
}

export function nedEuler(q: THREE.Quaternion) {
  const e = new THREE.Euler().setFromQuaternion(q, "ZYX");
  return { roll: e.x, pitch: e.y, yaw: e.z };
}

/** Earth's field direction (NED), assuming ~60° inclination toward north. */
export const EARTH_FIELD_NED = new THREE.Vector3(Math.cos(Math.PI / 3), 0, Math.sin(Math.PI / 3));

export type AttitudeSource = "vehicle" | "drag" | "phone" | "none";

const DOWN = new THREE.Vector3(0, 0, 1);

/**
 * The single source of truth for "where is the drone pointing right now".
 * It's read every frame by the scene, so it's a mutable singleton rather than
 * React state.
 */
class AttitudeState {
  /** Body → NED rotation. */
  readonly quat = new THREE.Quaternion();
  source: AttitudeSource = "none";
  lastUpdate = 0;
  /** Smoothed angular speed in rad/s, derived from successive samples. */
  angularSpeed = 0;
  /** Heading removed for display so the drone always faces the camera. */
  yawOffset = 0;

  private readonly previous = new THREE.Quaternion();
  private readonly scratch = new THREE.Quaternion();

  set(q: THREE.Quaternion, source: AttitudeSource) {
    const now = performance.now();
    const dt = (now - this.lastUpdate) / 1000;
    this.previous.copy(this.quat);
    this.quat.copy(q).normalize();
    if (this.lastUpdate > 0 && dt > 0 && dt < 0.5) {
      const angle = this.previous.angleTo(this.quat);
      const speed = angle / dt;
      this.angularSpeed += (speed - this.angularSpeed) * Math.min(1, dt * 6);
    }
    if (this.source === "none") this.yawOffset = nedEuler(this.quat).yaw;
    this.source = source;
    this.lastUpdate = now;
  }

  setEuler(roll: number, pitch: number, yaw: number, source: AttitudeSource) {
    this.set(nedQuat(roll, pitch, yaw, this.scratch), source);
  }

  /** Attitude in the scene frame with the heading offset removed. */
  displayQuat(out = new THREE.Quaternion()) {
    const heading = this.scratch.setFromAxisAngle(DOWN, -this.yawOffset);
    return nedQuatToThree(heading.multiply(this.quat), out);
  }

  /** Inverse of displayQuat: used when the user drags the drone around. */
  setFromDisplay(q3: THREE.Quaternion, source: AttitudeSource) {
    const ned = threeQuatToNed(q3, new THREE.Quaternion());
    const heading = new THREE.Quaternion().setFromAxisAngle(DOWN, this.yawOffset);
    this.set(heading.multiply(ned), source);
  }

  /** Point the display "forward" along the drone's current heading. */
  recenter() {
    this.yawOffset = nedEuler(this.quat).yaw;
  }

  /** Gravity as seen from the drone's body (unit vector, NED body axes). */
  gravityBody(out = new THREE.Vector3()) {
    return out.copy(DOWN).applyQuaternion(this.scratch.copy(this.quat).invert());
  }

  /** Earth's field direction in display (three.js) space. */
  fieldDisplay(out = new THREE.Vector3()) {
    const heading = this.scratch.setFromAxisAngle(DOWN, -this.yawOffset);
    const ned = EARTH_FIELD_NED.clone().applyQuaternion(heading);
    return nedToThree(ned, out);
  }

  tilt() {
    const { roll, pitch } = nedEuler(this.quat);
    return { roll, pitch };
  }

  isLive(maxAgeMs = 1500) {
    return this.source !== "none" && performance.now() - this.lastUpdate < maxAgeMs;
  }
}

export const attitude = new AttitudeState();

/**
 * Phone stand-in: DeviceOrientation (Z-X'-Y'' in an ENU earth frame, device
 * x right / y top / z out of screen). A phone lying screen-up with its top
 * pointing forward maps to a level drone.
 */
const PHONE_TO_BODY = new THREE.Matrix4().set(0, 1, 0, 0, 1, 0, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1);
export function phoneOrientationToNed(alpha: number, beta: number, gamma: number) {
  const d = THREE.MathUtils.degToRad;
  const enu = new THREE.Matrix4().makeRotationFromEuler(
    new THREE.Euler(d(beta), d(gamma), d(alpha), "ZXY"),
  );
  const ned = PHONE_TO_BODY.clone().multiply(enu).multiply(PHONE_TO_BODY.clone().transpose());
  return new THREE.Quaternion().setFromRotationMatrix(ned);
}
