import { useFrame } from "@react-three/fiber";
import { useEffect, useMemo, useRef, useState } from "react";
import * as THREE from "three";
import { SceneTunnel, showError, speak, useBackend, useCamera, useDrone, useVehicle } from "../app/state";
import { ActionBar, PadButton, Panel, ProgressRing, Title } from "../hud/Controls";
import { attitude, nedToThree } from "../lib/attitude";
import { type CoverageHint, type SpinAxis, SphereCoverage } from "../lib/coverage";
import { sfx } from "../lib/sfx";
import type { CompassStatus } from "../lib/types";
import { PulseRing, SpinArrow } from "../scene/fx";
import { palette, symbolColors } from "../theme";
import { ACTIVE_PHASES, DoneCard, FailCard, useCalibration } from "./common";
import { CompassMountSetup, OrientationSetup } from "./OrientationSetup";

const SPHERE_RADIUS = 1.05;
const AXIS_COLORS: Record<SpinAxis, string> = { yaw: palette.triangle, pitch: palette.circle, roll: palette.cross };
const DOWN = new THREE.Vector3(0, 0, 1);

const HINT_WORDS: Record<SpinAxis, [string, string]> = {
  roll: ["Roll it to the right!", "Roll it to the left!"],
  pitch: ["Tip the nose up!", "Tip the nose down!"],
  yaw: ["Spin it clockwise!", "Spin it counter-clockwise!"],
};

/** NED world vector → scene space, with the display heading removed. */
function worldToScene(v: THREE.Vector3, out = new THREE.Vector3()) {
  const heading = new THREE.Quaternion().setFromAxisAngle(DOWN, -attitude.yawOffset);
  return nedToThree(v.clone().applyQuaternion(heading), out);
}

/** A flat ring that fills (draw range) as an axis accumulates rotation. */
function AxisRing({ axis, fill, radius }: { axis: SpinAxis; fill: () => number; radius: number }) {
  const segments = 96;
  const geometry = useMemo(() => new THREE.RingGeometry(radius - 0.02, radius + 0.02, segments, 1), [radius]);
  useFrame(() => geometry.setDrawRange(0, Math.round(Math.min(1, fill()) * segments) * 6));
  // Ring planes in the body: yaw = horizontal, pitch = side plane, roll = front plane.
  const rotation: [number, number, number] =
    axis === "yaw" ? [-Math.PI / 2, 0, 0] : axis === "pitch" ? [0, Math.PI / 2, 0] : [0, 0, 0];
  return (
    <group rotation={rotation}>
      <mesh>
        <ringGeometry args={[radius - 0.012, radius + 0.012, segments, 1]} />
        <meshBasicMaterial color={palette.white} transparent opacity={0.12} side={THREE.DoubleSide} depthWrite={false} />
      </mesh>
      <mesh geometry={geometry}>
        <meshBasicMaterial color={AXIS_COLORS[axis]} toneMapped={false} side={THREE.DoubleSide} />
      </mesh>
    </group>
  );
}

function PaintSphere({ coverage, hint }: { coverage: SphereCoverage; hint: CoverageHint | null }) {
  const body = useRef<THREE.Group>(null);
  const cells = useRef<THREE.InstancedMesh>(null);
  const target = useRef<THREE.Group>(null);
  const goal = useMemo(() => new THREE.Quaternion(), []);
  const dummy = useMemo(() => new THREE.Object3D(), []);
  const positions = useMemo(
    () => coverage.cells.map((c) => nedToThree(c).multiplyScalar(SPHERE_RADIUS)),
    [coverage],
  );
  const idle = useMemo(() => new THREE.Color("#7aa6d8"), []);
  const colors = useMemo(() => symbolColors.map((c) => new THREE.Color(c)), []);

  useFrame(({ clock }, dt) => {
    coverage.update(attitude.quat);
    if (body.current) {
      attitude.displayQuat(goal);
      body.current.quaternion.slerp(goal, 1 - Math.exp(-10 * dt));
    }
    if (!cells.current) return;
    const now = performance.now();
    positions.forEach((p, i) => {
      const paintedAt = coverage.paintedAt[i];
      const age = paintedAt ? (now - paintedAt) / 1000 : -1;
      // Freshly painted cells pop, then settle.
      const pop = age >= 0 && age < 0.5 ? Math.sin((age / 0.5) * Math.PI) * 0.9 : 0;
      dummy.position.copy(p);
      dummy.lookAt(p.clone().multiplyScalar(2));
      const isTarget = hint?.target === i;
      dummy.scale.setScalar(paintedAt ? 1 + pop : isTarget ? 1.2 + Math.sin(clock.elapsedTime * 8) * 0.3 : 0.55);
      dummy.updateMatrix();
      cells.current!.setMatrixAt(i, dummy.matrix);
      cells.current!.setColorAt(i, paintedAt ? colors[i % colors.length] : isTarget ? new THREE.Color(palette.gold) : idle);
    });
    cells.current.instanceMatrix.needsUpdate = true;
    if (cells.current.instanceColor) cells.current.instanceColor.needsUpdate = true;
    if (target.current && hint) target.current.position.copy(positions[hint.target]);
  });

  return (
    <group ref={body}>
      <mesh>
        <icosahedronGeometry args={[SPHERE_RADIUS * 0.985, 2]} />
        <meshBasicMaterial color={palette.glow} wireframe transparent opacity={0.08} depthWrite={false} />
      </mesh>
      <instancedMesh ref={cells} args={[undefined, undefined, coverage.cells.length]} frustumCulled={false}>
        <circleGeometry args={[0.075, 6]} />
        <meshBasicMaterial toneMapped={false} side={THREE.DoubleSide} />
      </instancedMesh>
      <AxisRing axis="yaw" radius={0.7} fill={() => coverage.spin.yaw / (Math.PI * 2)} />
      <AxisRing axis="pitch" radius={0.74} fill={() => coverage.spin.pitch / (Math.PI * 2)} />
      <AxisRing axis="roll" radius={0.78} fill={() => coverage.spin.roll / (Math.PI * 2)} />
      {hint && (
        <group ref={target}>
          <PulseRing radius={0.14} color={palette.gold} speed={1.5} rotation={[Math.PI / 2, 0, 0]} />
        </group>
      )}
    </group>
  );
}

/** The fixed "brush": Earth's field, which paints whatever passes under it. */
function FieldBrush() {
  const group = useRef<THREE.Group>(null);
  const beam = useRef<THREE.Mesh>(null);
  useFrame(() => {
    const dir = attitude.fieldDisplay();
    group.current?.position.copy(dir.clone().multiplyScalar(SPHERE_RADIUS + 0.02));
    if (beam.current) {
      beam.current.position.copy(dir.clone().multiplyScalar(SPHERE_RADIUS * 0.5));
      beam.current.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), dir);
    }
  });
  return (
    <>
      <mesh ref={beam}>
        <cylinderGeometry args={[0.008, 0.008, SPHERE_RADIUS, 8]} />
        <meshBasicMaterial color={palette.gold} transparent opacity={0.5} toneMapped={false} />
      </mesh>
      <group ref={group}>
        <mesh>
          <sphereGeometry args={[0.09, 24, 24]} />
          <meshStandardMaterial color={palette.gold} emissive={palette.gold} emissiveIntensity={2.4} toneMapped={false} />
        </mesh>
        <pointLight color={palette.gold} intensity={1.5} distance={1.5} />
      </group>
    </>
  );
}

/** White arrow around the axis the user is spinning right now. */
function CurrentSpin() {
  const axis = useMemo(() => new THREE.Vector3(0, 1, 0), []);
  const previous = useMemo(() => attitude.displayQuat(), []);
  const [strength, setStrength] = useState(0);
  const smooth = useRef(0);
  useFrame((_, dt) => {
    const now = attitude.displayQuat();
    const delta = now.clone().multiply(previous.clone().invert());
    previous.copy(now);
    if (delta.w < 0) delta.set(-delta.x, -delta.y, -delta.z, -delta.w);
    const angle = 2 * Math.acos(Math.min(1, delta.w));
    const speed = dt > 0 ? angle / dt : 0;
    if (angle > 1e-4) axis.lerp(new THREE.Vector3(delta.x, delta.y, delta.z).normalize(), 0.25).normalize();
    smooth.current = THREE.MathUtils.damp(smooth.current, Math.min(1, speed / 2), 5, dt);
    const bucket = Math.round(smooth.current * 10) / 10;
    if (bucket !== strength) setStrength(bucket);
  });
  if (strength < 0.1) return null;
  return <SpinArrow axis={axis} radius={1.2} color={palette.white} opacity={strength} arc={Math.PI * 0.7} speed={2.5} tube={0.02} />;
}

function HintArrow({ hint }: { hint: CoverageHint }) {
  const axis = useMemo(() => worldToScene(hint.axisWorld), [hint]);
  return <SpinArrow axis={axis} radius={1.3} color={palette.gold} arc={Math.PI * 0.55} tube={0.032} />;
}

type Phase = "orient" | "mount" | "ready" | "running" | "done" | "failed";

export function CompassStep() {
  const backend = useBackend();
  const vehicle = useVehicle();
  const px4 = vehicle.firmware === "px4";
  const generic = useCalibration("compass");
  const [phase, setPhase] = useState<Phase>("orient");
  const [status, setStatus] = useState<CompassStatus | null>(null);
  const [hint, setHint] = useState<CoverageHint | null>(null);
  const [painted, setPainted] = useState(0);
  const [spin, setSpin] = useState({ roll: 0, pitch: 0, yaw: 0 });
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const coverage = useMemo(() => new SphereCoverage(), []);

  useDrone({ pose: "live", visible: phase !== "orient" && phase !== "mount" });
  useCamera(
    phase === "running" ? [0.3, 1.25, 4.9] : [0.6, 1.2, 4.2],
    [0, -0.05, 0],
    40,
    phase !== "orient" && phase !== "mount",
  );

  useEffect(() => backend.onCompass(setStatus), [backend]);

  useEffect(() => {
    if (phase === "ready") speak("Spin it all the way around — every side, every direction!");
  }, [phase]);

  // Refresh the hint + HUD numbers a few times a second (not every frame).
  useEffect(() => {
    if (phase !== "running") return;
    const timer = window.setInterval(() => {
      const next = coverage.hint(attitude.quat);
      setHint((current) => (current?.target === next?.target ? current : next));
      setPainted(coverage.fraction());
      setSpin({ ...coverage.spin });
    }, 450);
    return () => window.clearInterval(timer);
  }, [phase, coverage]);

  useEffect(() => {
    if (phase === "running" && hint) speak(HINT_WORDS[hint.spin][hint.sign === 1 ? 0 : 1]);
  }, [hint?.spin, hint?.sign, phase]);

  // Firmware result is authoritative.
  const result = px4 ? generic?.phase : status?.status;
  useEffect(() => {
    if (phase !== "running" || !result) return;
    if (result === "completed") {
      sfx.success();
      setPhase("done");
    } else if (["failed", "stopped", "unknown"].includes(result)) {
      const reason = status?.sensors.find((s) => s.failure_reason)?.failure_reason;
      setFailure(reason ?? status?.instruction ?? generic?.message ?? "Compass calibration did not finish.");
      setPhase("failed");
    }
  }, [result, phase]);

  const start = async () => {
    setBusy(true);
    try {
      attitude.recenter();
      coverage.reset();
      setHint(null);
      if (px4) await backend.startCalibration("compass");
      else setStatus(await backend.startCompass({ compass_ids: [], retry_on_failure: false, autosave: true }));
      setPhase("running");
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  const cancel = async () => {
    try {
      if (px4) await backend.stopCalibration();
      else if (status) await backend.cancelCompass(status.session_id);
    } catch (error) {
      showError(error);
    }
    setPhase("ready");
  };

  const firmwarePercent = px4
    ? generic?.percent ?? null
    : status?.sensors.length
      ? status.sensors.reduce((sum, s) => sum + s.progress_percent, 0) / status.sensors.length
      : null;

  if (phase === "orient") return <OrientationSetup sensorName="Compass" onConfirm={() => setPhase("mount")} />;
  if (phase === "mount") return <CompassMountSetup onBack={() => setPhase("orient")} onConfirm={() => setPhase("ready")} />;
  if (phase === "failed") return <FailCard message={failure} onRetry={() => setPhase("ready")} />;
  if (phase === "done") {
    return (
      <DoneCard step="compass" title="Compass is pointing true!" onRedo={() => setPhase("ready")}>
        {status?.sensors.map((sensor) => <FitnessBar key={sensor.compass_id} id={sensor.compass_id} fitness={sensor.fitness} />)}
        <p className="hint hint--warn">Reboot your drone to apply the new compass offsets.</p>
      </DoneCard>
    );
  }

  return (
    <>
      <SceneTunnel.In>
        {(phase === "running" || phase === "ready") && <PaintSphere coverage={coverage} hint={phase === "running" ? hint : null} />}
        <FieldBrush />
        {phase === "running" && <CurrentSpin />}
        {phase === "running" && hint && <HintArrow hint={hint} />}
      </SceneTunnel.In>

      {phase === "ready" ? (
        <Panel className="panel--left">
          <Title kicker="Compass">Paint the sphere</Title>
          <p className="hint">Away from metal and laptops. Spin the drone so the gold spark paints every dot.</p>
          <AxisLegend spin={spin} />
        </Panel>
      ) : (
        <div className="side-hud">
          <ProgressRing value={firmwarePercent !== null ? firmwarePercent / 100 : null} color={palette.cross} size={112} />
          <div className="side-hud__caption">Sphere {Math.round(painted * 100)}%</div>
          <AxisLegend spin={spin} />
        </div>
      )}

      <ActionBar>
        {phase === "ready" ? (
          <PadButton glyph="cross" primary busy={busy} onPress={start}>
            Start spinning
          </PadButton>
        ) : (
          ACTIVE_PHASES.includes(result ?? "in_progress") && (
            <PadButton glyph="circle" onPress={cancel}>
              Cancel
            </PadButton>
          )
        )}
      </ActionBar>
    </>
  );
}

function AxisLegend({ spin }: { spin: Record<SpinAxis, number> }) {
  return (
    <div className="axis-legend">
      {(["yaw", "pitch", "roll"] as SpinAxis[]).map((axis) => {
        const fill = Math.min(1, spin[axis] / (Math.PI * 2));
        return (
          <div key={axis} className="axis-legend__item" style={{ "--axis": AXIS_COLORS[axis], "--fill": fill } as React.CSSProperties}>
            <span className="axis-legend__ring" />
            <span>{axis}</span>
            <small>{Math.round(fill * 360)}°</small>
          </div>
        );
      })}
    </div>
  );
}

/** QGroundControl's fitness bar: green < 8, yellow < 15, red beyond (doubled for internal). */
function FitnessBar({ id, fitness }: { id: number; fitness: number | null }) {
  const range = 25;
  const position = fitness === null ? null : Math.min(1, Math.max(0, fitness / range));
  return (
    <div className="fitness">
      <span>Compass {id + 1}</span>
      <div className="fitness__bar">
        <i style={{ flex: 8 }} className="fitness__green" />
        <i style={{ flex: 7 }} className="fitness__yellow" />
        <i style={{ flex: 10 }} className="fitness__red" />
        {position !== null && <b style={{ left: `${position * 100}%` }} />}
      </div>
      <small>{fitness === null ? "no report" : fitness.toFixed(1)}</small>
    </div>
  );
}
