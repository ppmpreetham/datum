import { useFrame } from "@react-three/fiber";
import clsx from "clsx";
import { useEffect, useMemo, useRef, useState } from "react";
import * as THREE from "three";
import { SceneTunnel, showError, speak, useBackend, useCamera, useDrone } from "../app/state";
import { ActionBar, PadButton, Panel, Title } from "../hud/Controls";
import { attitude, nedQuat, nedQuatToThree } from "../lib/attitude";
import { sfx } from "../lib/sfx";
import type { AccelPosition, AccelerometerStatus } from "../lib/types";
import { PulseRing, SpinArrow } from "../scene/fx";
import { Drone } from "../scene/models/Drone";
import { palette } from "../theme";
import { DoneCard, FailCard, useHeld } from "./common";
import { OrientationSetup } from "./OrientationSetup";

const d = THREE.MathUtils.degToRad;

/** Each pose: target attitude (NED) and where gravity points in the body. */
export const POSES: Record<
  AccelPosition,
  { label: string; say: string; roll: number; pitch: number; gravity: THREE.Vector3; color: string; icon: number }
> = {
  level: { label: "Flat", say: "Sit it flat on the table.", roll: 0, pitch: 0, gravity: new THREE.Vector3(0, 0, 1), color: palette.triangle, icon: 0 },
  left: { label: "Left side", say: "Roll it onto its left side.", roll: d(-90), pitch: 0, gravity: new THREE.Vector3(0, -1, 0), color: palette.circle, icon: -90 },
  right: { label: "Right side", say: "Now onto its right side.", roll: d(90), pitch: 0, gravity: new THREE.Vector3(0, 1, 0), color: palette.cross, icon: 90 },
  nose_down: { label: "Nose down", say: "Stand it on its nose.", roll: 0, pitch: d(-90), gravity: new THREE.Vector3(1, 0, 0), color: palette.square, icon: 90 },
  nose_up: { label: "Nose up", say: "Stand it on its tail, nose up.", roll: 0, pitch: d(90), gravity: new THREE.Vector3(-1, 0, 0), color: palette.gold, icon: -90 },
  upside_down: { label: "Upside down", say: "Last one — flip it upside down!", roll: d(180), pitch: 0, gravity: new THREE.Vector3(0, 0, -1), color: palette.cyan, icon: 180 },
};
const ORDER: AccelPosition[] = ["level", "left", "right", "nose_down", "nose_up", "upside_down"];
const MATCH = d(14);

function poseQuat(pose: AccelPosition) {
  return nedQuatToThree(nedQuat(POSES[pose].roll, POSES[pose].pitch, 0));
}

function poseError(pose: AccelPosition) {
  return attitude.gravityBody().angleTo(POSES[pose].gravity);
}

/** Glowing beads flowing from the live drone's nose to the ghost's nose. */
function GuideBeads({ pose, color }: { pose: AccelPosition; color: string }) {
  const mesh = useRef<THREE.InstancedMesh>(null);
  const live = useMemo(() => new THREE.Quaternion(), []);
  const target = useMemo(() => poseQuat(pose), [pose]);
  const dummy = useMemo(() => new THREE.Object3D(), []);
  const count = 26;
  useFrame(({ clock }) => {
    if (!mesh.current) return;
    attitude.displayQuat(live);
    const nose = new THREE.Vector3(0, 0, -0.62);
    const from = nose.clone().applyQuaternion(live);
    const to = nose.clone().applyQuaternion(target);
    // Bulge outward so the path reads as a swing, not a straight cut.
    const mid = from.clone().add(to).multiplyScalar(0.5);
    mid.add(mid.clone().normalize().multiplyScalar(0.55 + from.distanceTo(to) * 0.25));
    const curve = new THREE.QuadraticBezierCurve3(from, mid, to);
    const visible = from.distanceTo(to) > 0.2;
    for (let i = 0; i < count; i++) {
      const t = (i / count + clock.elapsedTime * 0.35) % 1;
      dummy.position.copy(curve.getPoint(t));
      dummy.scale.setScalar(visible ? 0.012 + t * 0.022 : 0);
      dummy.updateMatrix();
      mesh.current.setMatrixAt(i, dummy.matrix);
    }
    mesh.current.instanceMatrix.needsUpdate = true;
  });
  return (
    <instancedMesh ref={mesh} args={[undefined, undefined, count]} frustumCulled={false}>
      <sphereGeometry args={[1, 10, 10]} />
      <meshBasicMaterial color={color} toneMapped={false} />
    </instancedMesh>
  );
}

/** Rotation arrow showing the turn from where it is to where it should be. */
function TurnHint({ pose, color }: { pose: AccelPosition; color: string }) {
  const axis = useMemo(() => new THREE.Vector3(), []);
  const [visible, setVisible] = useState(true);
  const live = useMemo(() => new THREE.Quaternion(), []);
  const target = useMemo(() => poseQuat(pose), [pose]);
  useFrame(() => {
    attitude.displayQuat(live);
    const delta = target.clone().multiply(live.clone().invert());
    if (delta.w < 0) delta.set(-delta.x, -delta.y, -delta.z, -delta.w);
    const angle = 2 * Math.acos(Math.min(1, delta.w));
    axis.set(delta.x, delta.y, delta.z);
    if (axis.lengthSq() > 1e-8) axis.normalize();
    const show = angle > MATCH * 1.5;
    if (show !== visible) setVisible(show);
  });
  return visible ? <SpinArrow axis={axis} radius={0.82} color={color} arc={Math.PI * 0.55} tube={0.026} /> : null;
}

function GhostTarget({ pose, matched }: { pose: AccelPosition; matched: boolean }) {
  const target = useRef(poseQuat(pose));
  useEffect(() => {
    target.current = poseQuat(pose);
  }, [pose]);
  const color = matched ? palette.triangle : POSES[pose].color;
  return (
    <group>
      <Drone ghost={color} opacity={matched ? 1 : 0.75} target={target} follow={5} />
      {matched && <PulseRing radius={0.9} color={palette.triangle} speed={1.4} position={[0, -0.3, 0]} />}
    </group>
  );
}

/** Six little pose tiles: done / now / later. */
function PoseTiles({ done, current }: { done: AccelPosition[]; current: AccelPosition | null }) {
  return (
    <div className="pose-tiles">
      {ORDER.map((pose) => {
        const p = POSES[pose];
        const side = pose === "nose_up" || pose === "nose_down";
        return (
          <div
            key={pose}
            className={clsx("pose-tile", done.includes(pose) && "is-done", current === pose && "is-current")}
            style={{ "--tile": p.color } as React.CSSProperties}
          >
            <svg viewBox="-20 -20 40 40" width="34" height="34">
              <g transform={`rotate(${p.icon})`}>
                {side ? (
                  <>
                    <rect x="-12" y="-2" width="24" height="5" rx="2.5" fill="currentColor" />
                    <path d="M12 -1 l5 1.5 l-5 1.5z" fill="currentColor" />
                    <rect x="-15" y="-6" width="7" height="2" rx="1" fill="currentColor" />
                    <rect x="8" y="-6" width="7" height="2" rx="1" fill="currentColor" />
                  </>
                ) : (
                  <>
                    <rect x="-8" y="-2" width="16" height="6" rx="3" fill="currentColor" />
                    <rect x="-17" y="-6" width="10" height="2" rx="1" fill="currentColor" />
                    <rect x="7" y="-6" width="10" height="2" rx="1" fill="currentColor" />
                    <circle cx="0" cy="-5" r="1.6" fill="currentColor" />
                  </>
                )}
              </g>
            </svg>
            <span>{p.label}</span>
          </div>
        );
      })}
    </div>
  );
}

type Phase = "orient" | "ready" | "running" | "done" | "failed";

export function AccelStep() {
  const backend = useBackend();
  const [phase, setPhase] = useState<Phase>("orient");
  const [status, setStatus] = useState<AccelerometerStatus | null>(null);
  const [done, setDone] = useState<AccelPosition[]>([]);
  const [busy, setBusy] = useState(false);
  const [matched, setMatched] = useState(false);
  const [failure, setFailure] = useState("");
  const confirming = useRef(false);

  const pose = status?.waiting_for_confirmation ? status.orientation : null;

  useDrone({ pose: phase === "running" || phase === "ready" ? "live" : "level", visible: phase !== "orient" });
  useCamera([0.4, 1.0, 3.1], [0, 0, 0], 40, phase !== "orient");

  useEffect(() => backend.onAccelerometer(setStatus), [backend]);

  useEffect(() => {
    if (phase === "ready") speak("Six poses. Match the ghost each time!");
  }, [phase]);

  useEffect(() => {
    if (phase !== "running" || !status) return;
    if (status.status === "completed") setPhase("done");
    else if (["failed", "stopped", "unknown"].includes(status.status)) {
      setFailure(status.instruction);
      setPhase("failed");
    } else if (pose) {
      speak(POSES[pose].say);
    } else if (status.status === "in_progress") {
      speak("Hold it… hold it…");
    }
  }, [status?.status, pose, phase]);

  // Match detection for the current pose.
  useEffect(() => {
    if (!pose) return;
    const timer = window.setInterval(() => {
      const ok = poseError(pose) < MATCH;
      setMatched((was) => {
        if (ok && !was) sfx.pop(4);
        return ok;
      });
    }, 80);
    return () => window.clearInterval(timer);
  }, [pose]);

  const confirm = async () => {
    if (!status || !pose || confirming.current) return;
    confirming.current = true;
    try {
      const next = await backend.confirmAccelerometerPosition(status.session_id, pose);
      sfx.ding();
      setDone((current) => (current.includes(pose) ? current : [...current, pose]));
      setStatus(next);
      setMatched(false);
    } catch (error) {
      showError(error);
    } finally {
      confirming.current = false;
    }
  };

  const held = useHeld(() => !!pose && poseError(pose) < MATCH && attitude.angularSpeed < 0.2, !!pose);
  useEffect(() => {
    if (held > 1.2) void confirm();
  }, [held > 1.2]);

  const start = async () => {
    setBusy(true);
    try {
      attitude.recenter();
      setDone([]);
      const first = await backend.startAccelerometer();
      setStatus(first);
      setPhase("running");
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  const cancel = async () => {
    if (!status) return;
    try {
      await backend.cancelAccelerometer(status.session_id);
    } catch (error) {
      showError(error);
    }
    setPhase("ready");
  };

  if (phase === "orient") return <OrientationSetup sensorName="Accelerometer" onConfirm={() => setPhase("ready")} />;
  if (phase === "done") return <DoneCard step="accel" title="All six sides — nailed it!" onRedo={() => setPhase("ready")} />;
  if (phase === "failed") return <FailCard message={failure} onRetry={() => setPhase("ready")} />;

  const color = pose ? POSES[pose].color : palette.glow;
  const holdFill = Math.min(1, held / 1.2);

  return (
    <>
      <SceneTunnel.In>
        {pose && (
          <>
            <GhostTarget pose={pose} matched={matched} />
            {!matched && <GuideBeads pose={pose} color={color} />}
            {!matched && <TurnHint pose={pose} color={color} />}
          </>
        )}
      </SceneTunnel.In>

      {phase === "ready" ? (
        <Panel className="panel--left">
          <Title kicker="Accelerometer">The six-side pose</Title>
          <p className="hint">Props off, on a flat table. I'll show a ghost — slide your drone into it.</p>
          <PoseTiles done={[]} current={null} />
        </Panel>
      ) : (
        <>
          <PoseTiles done={done} current={pose} />
          {pose && (
            <div className="hold-meter" style={{ "--fill": holdFill, "--tile": matched ? palette.triangle : color } as React.CSSProperties}>
              <span>{matched ? "Hold still…" : POSES[pose].label}</span>
            </div>
          )}
        </>
      )}

      <ActionBar>
        {phase === "ready" ? (
          <PadButton glyph="cross" primary busy={busy} onPress={start}>
            Start
          </PadButton>
        ) : (
          <>
            <PadButton glyph="circle" onPress={cancel}>
              Cancel
            </PadButton>
            <PadButton glyph="cross" primary disabled={!pose} onPress={confirm}>
              {matched ? "Got it" : "It's in place"}
            </PadButton>
          </>
        )}
      </ActionBar>
    </>
  );
}
