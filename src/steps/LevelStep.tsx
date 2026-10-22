import { useFrame } from "@react-three/fiber";
import { useEffect, useMemo, useRef, useState } from "react";
import * as THREE from "three";
import { SceneTunnel, app, showError, speak, useBackend, useCamera, useDrone } from "../app/state";
import { ActionBar, PadButton, ProgressRing } from "../hud/Controls";
import { attitude } from "../lib/attitude";
import { sfx } from "../lib/sfx";
import { palette } from "../theme";
import { ACTIVE_PHASES, DoneCard, FailCard, useCalibration, useHeld } from "./common";
import { OrientationSetup } from "./OrientationSetup";

const DISH_RADIUS = 0.6;
/** Tilt that pushes the bubble to the rim. Small, so tiny offsets show. */
const FULL_SCALE = THREE.MathUtils.degToRad(6);
/** ArduPilot refuses to trim beyond ~10°. */
const MAX_TILT = THREE.MathUtils.degToRad(10);
const CENTER = THREE.MathUtils.degToRad(0.6);

function bubbleOffset() {
  const { roll, pitch } = attitude.tilt();
  // A bubble floats to the high side: roll right → left, nose up → forward.
  let x = (-roll / FULL_SCALE) * DISH_RADIUS * 0.8;
  let z = (-pitch / FULL_SCALE) * DISH_RADIUS * 0.8;
  const r = Math.hypot(x, z);
  const max = DISH_RADIUS * 0.82;
  if (r > max) {
    x *= max / r;
    z *= max / r;
  }
  return { x, z };
}

function BubbleLevel({ settled }: { settled: boolean }) {
  const bubble = useRef<THREE.Mesh>(null);
  const velocity = useRef(new THREE.Vector2());
  const position = useRef(new THREE.Vector2());
  const material = useRef<THREE.MeshStandardMaterial>(null);
  const target = useRef<THREE.Mesh>(null);
  const gold = useMemo(() => new THREE.Color(palette.gold), []);
  const centered = useMemo(() => new THREE.Color(palette.triangle), []);
  const off = useMemo(() => new THREE.Color(palette.cyan), []);

  useFrame(({ clock }, dt) => {
    if (!bubble.current) return;
    const step = Math.min(dt, 0.05);
    const goal = bubbleOffset();
    // Springy, slightly underdamped motion: it should feel like liquid.
    velocity.current.x += ((goal.x - position.current.x) * 60 - velocity.current.x * 9) * step;
    velocity.current.y += ((goal.z - position.current.y) * 60 - velocity.current.y * 9) * step;
    position.current.addScaledVector(velocity.current, step);
    bubble.current.position.set(position.current.x, 0.05, position.current.y);
    const wobble = 1 + velocity.current.length() * 0.15;
    bubble.current.scale.set(wobble, 0.55, 1 / wobble);
    const nearCenter = position.current.length() < 0.06;
    material.current?.color.lerp(nearCenter ? centered : off, 0.15);
    material.current?.emissive.lerp(nearCenter ? centered : off, 0.15);
    if (material.current) material.current.emissiveIntensity = 1.2 + Math.sin(clock.elapsedTime * 3) * 0.2;
    // The centre target turns green and breathes once the drone has settled.
    if (target.current) {
      (target.current.material as THREE.MeshBasicMaterial).color.lerp(settled ? centered : gold, 0.1);
      target.current.scale.setScalar(settled ? 1 + Math.sin(clock.elapsedTime * 5) * 0.12 : 1);
    }
  });

  return (
    <group position={[1.35, 0.2, 0.1]} rotation={[0.55, -0.35, 0]} scale={0.85}>
      <mesh>
        <cylinderGeometry args={[DISH_RADIUS + 0.05, DISH_RADIUS + 0.08, 0.06, 64]} />
        <meshStandardMaterial color={palette.plastic} roughness={0.25} />
      </mesh>
      <mesh position={[0, 0.032, 0]}>
        <cylinderGeometry args={[DISH_RADIUS, DISH_RADIUS, 0.005, 64]} />
        <meshStandardMaterial color={palette.deep} roughness={0.2} />
      </mesh>
      <mesh ref={target} position={[0, 0.037, 0]} rotation={[-Math.PI / 2, 0, 0]}>
        <ringGeometry args={[0.114, 0.126, 64]} />
        <meshBasicMaterial color={palette.gold} toneMapped={false} />
      </mesh>
      {[0.3, 0.48].map((r) => (
        <mesh key={r} position={[0, 0.037, 0]} rotation={[-Math.PI / 2, 0, 0]}>
          <ringGeometry args={[r - 0.006, r + 0.006, 64]} />
          <meshBasicMaterial color={palette.white} transparent opacity={0.35} />
        </mesh>
      ))}
      {[0, Math.PI / 2].map((angle) => (
        <mesh key={angle} position={[0, 0.037, 0]} rotation={[-Math.PI / 2, 0, angle]}>
          <planeGeometry args={[DISH_RADIUS * 2, 0.008]} />
          <meshBasicMaterial color={palette.white} transparent opacity={0.3} />
        </mesh>
      ))}
      <mesh ref={bubble}>
        <sphereGeometry args={[0.085, 32, 32]} />
        <meshStandardMaterial ref={material} color={palette.cyan} emissive={palette.cyan} roughness={0.1} toneMapped={false} />
      </mesh>
      {/* Glass dome */}
      <mesh position={[0, 0.03, 0]} scale={[1, 0.18, 1]}>
        <sphereGeometry args={[DISH_RADIUS, 48, 16, 0, Math.PI * 2, 0, Math.PI / 2]} />
        <meshPhysicalMaterial color={palette.white} transparent opacity={0.16} roughness={0} clearcoat={1} depthWrite={false} />
      </mesh>
    </group>
  );
}

type Phase = "orient" | "align" | "running" | "done" | "failed";

export function LevelStep() {
  const backend = useBackend();
  const calibration = useCalibration("level");
  const [phase, setPhase] = useState<Phase>("orient");
  const [tilt, setTilt] = useState({ roll: 0, pitch: 0 });
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const baseline = useRef<unknown>(null);

  useDrone({ pose: "live", visible: phase !== "orient" });
  useCamera([0.55, 1.5, 3.4], [0.55, 0.1, 0], 40, phase !== "orient");

  useEffect(() => {
    if (phase !== "align" && phase !== "running") return;
    const timer = window.setInterval(() => setTilt(attitude.tilt()), 120);
    return () => window.clearInterval(timer);
  }, [phase]);

  const steady = () => {
    const { roll, pitch } = attitude.tilt();
    return Math.hypot(roll, pitch) < MAX_TILT && attitude.angularSpeed < 0.12;
  };
  const held = useHeld(steady, phase === "align");
  const ready = held > 1;
  const tooTilted = Math.hypot(tilt.roll, tilt.pitch) >= MAX_TILT;
  const centered = Math.hypot(tilt.roll, tilt.pitch) < CENTER;

  useEffect(() => {
    if (phase !== "align") return;
    if (tooTilted) speak("Whoa, that's tilted. Find a flatter spot.", "warn");
    else if (ready) speak(centered ? "Already centered! Lock it in." : "Nice and still. Level it!");
    else speak("Set it down in its flying position, nice and flat.");
  }, [phase, tooTilted, ready]);

  useEffect(() => {
    if (phase !== "running" || !calibration || calibration === baseline.current) return;
    if (calibration.phase === "completed") {
      sfx.success();
      setPhase("done");
    } else if (["failed", "stopped", "unknown"].includes(calibration.phase)) {
      setFailure(calibration.message);
      setPhase("failed");
    }
  }, [calibration?.phase, phase]);

  const level = async () => {
    setBusy(true);
    baseline.current = app.get().vehicle.calibration;
    try {
      await backend.startCalibration("level");
      speak("Adjusting trims…");
      setPhase("running");
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  const deg = (v: number) => `${(THREE.MathUtils.radToDeg(v) >= 0 ? "+" : "") + THREE.MathUtils.radToDeg(v).toFixed(1)}°`;

  if (phase === "orient") return <OrientationSetup sensorName="Level horizon" onConfirm={() => setPhase("align")} />;
  if (phase === "failed") return <FailCard message={failure} onRetry={() => setPhase("align")} />;

  return (
    <>
      <SceneTunnel.In>
        <BubbleLevel settled={phase === "done" || (phase === "align" && ready && centered)} />
      </SceneTunnel.In>

      {phase === "done" ? (
        <DoneCard step="level" title="Horizon is dead level!" onRedo={() => setPhase("align")} />
      ) : (
        <>
          <div className="tilt-chips">
            <span className="tilt-chip">Roll {deg(tilt.roll)}</span>
            <span className="tilt-chip">Pitch {deg(tilt.pitch)}</span>
          </div>
          {phase === "running" && (
            <div className="center-ring">
              <ProgressRing value={calibration?.percent != null ? calibration.percent / 100 : null} color={palette.gold} />
            </div>
          )}
          <ActionBar>
            {phase === "align" ? (
              <PadButton glyph="cross" primary busy={busy} disabled={!ready || tooTilted} onPress={level}>
                Level it!
              </PadButton>
            ) : (
              ACTIVE_PHASES.includes(calibration?.phase ?? "") && (
                <PadButton glyph="circle" onPress={() => backend.stopCalibration().catch(showError)}>
                  Cancel
                </PadButton>
              )
            )}
          </ActionBar>
        </>
      )}
    </>
  );
}
