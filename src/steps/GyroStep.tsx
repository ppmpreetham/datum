import { useFrame } from "@react-three/fiber";
import clsx from "clsx";
import { useEffect, useMemo, useRef, useState } from "react";
import * as THREE from "three";
import { SceneTunnel, showError, speak, useBackend, useCamera, useDrone } from "../app/state";
import { ActionBar, PadButton, ProgressRing } from "../hud/Controls";
import { attitude } from "../lib/attitude";
import { sfx } from "../lib/sfx";
import { SpinArrow, TargetOrb } from "../scene/fx";
import { palette } from "../theme";
import { ACTIVE_PHASES, ArmTrails, DoneCard, FailCard, useCalibration, useHeld } from "./common";
import { OrientationSetup } from "./OrientationSetup";

const TILT = THREE.MathUtils.degToRad(22);

/** Wake-up targets: tilt toward each orb. Axis = scene-space turn that gets there. */
const ORBS = [
  { id: "front", say: "Tip the nose down!", position: [0, -0.1, -1.15], axis: new THREE.Vector3(-1, 0, 0), color: palette.triangle, hit: (_: number, p: number) => p < -TILT },
  { id: "right", say: "Now lean right!", position: [1.15, -0.1, 0], axis: new THREE.Vector3(0, 0, -1), color: palette.circle, hit: (r: number) => r > TILT },
  { id: "back", say: "Nose up!", position: [0, -0.1, 1.15], axis: new THREE.Vector3(1, 0, 0), color: palette.cross, hit: (_: number, p: number) => p > TILT },
  { id: "left", say: "And lean left!", position: [-1.15, -0.1, 0], axis: new THREE.Vector3(0, 0, 1), color: palette.square, hit: (r: number) => r < -TILT },
] as const;

const RING_SEGMENTS = 128;

type Phase = "orient" | "wake" | "settle" | "running" | "done" | "failed";

/** Ring around the drone that fills with progress and shivers on motion. */
function StillnessRing({ progress }: { progress: number | null }) {
  const group = useRef<THREE.Group>(null);
  const fill = useRef<THREE.Mesh>(null);
  const shown = useRef(0);
  const geometry = useMemo(() => new THREE.RingGeometry(0.9, 1.0, RING_SEGMENTS, 1), []);
  useFrame(({ clock }, dt) => {
    if (!group.current || !fill.current) return;
    const moving = attitude.angularSpeed > 0.3;
    group.current.position.x = moving ? Math.sin(clock.elapsedTime * 60) * 0.02 : 0;
    const goal = progress ?? (Math.sin(clock.elapsedTime * 2) * 0.5 + 0.5) * 0.3;
    shown.current = THREE.MathUtils.damp(shown.current, goal, 4, dt);
    geometry.setDrawRange(0, Math.round(shown.current * RING_SEGMENTS) * 6);
    if (progress === null) fill.current.rotation.z = clock.elapsedTime * 2;
    (fill.current.material as THREE.MeshBasicMaterial).color.set(moving ? palette.circle : palette.triangle);
  });
  return (
    <group ref={group} position={[0, -0.25, 0]} rotation={[-Math.PI / 2, 0, 0]}>
      <mesh>
        <ringGeometry args={[0.9, 1.0, RING_SEGMENTS, 1]} />
        <meshBasicMaterial color={palette.white} transparent opacity={0.18} depthWrite={false} side={THREE.DoubleSide} />
      </mesh>
      <mesh ref={fill} geometry={geometry} position={[0, 0, 0.001]}>
        <meshBasicMaterial color={palette.triangle} toneMapped={false} side={THREE.DoubleSide} />
      </mesh>
    </group>
  );
}

export function GyroStep() {
  const backend = useBackend();
  const calibration = useCalibration("gyro");
  const [phase, setPhase] = useState<Phase>("orient");
  const [reached, setReached] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const started = useRef(false);

  useDrone({ pose: phase === "orient" ? "level" : "live", visible: phase !== "orient" });
  useCamera(phase === "wake" ? [1.5, 1.9, 2.9] : [0.4, 1.1, 3.0], [0, -0.1, 0], 40, phase !== "orient");

  const nextOrb = ORBS.find((o) => !reached.includes(o.id));

  // Wake-up: watch the tilt and pop orbs as they're reached.
  useEffect(() => {
    if (phase !== "wake") return;
    attitude.recenter();
    const timer = window.setInterval(() => {
      const { roll, pitch } = attitude.tilt();
      const hit = ORBS.find((o) => o.hit(roll, pitch));
      if (hit) {
        setReached((current) => {
          if (current.includes(hit.id)) return current;
          sfx.pop(current.length * 2);
          return [...current, hit.id];
        });
      }
    }, 60);
    return () => window.clearInterval(timer);
  }, [phase]);

  useEffect(() => {
    if (phase !== "wake") return;
    if (nextOrb) speak(reached.length === 0 ? `Let's wake up the gyro. ${nextOrb.say}` : nextOrb.say);
    else {
      sfx.ding();
      setPhase("settle");
    }
  }, [phase, nextOrb?.id]);

  useEffect(() => {
    if (phase === "settle") speak("Now set it down flat… and freeze!");
  }, [phase]);

  const flatAndStill = () => {
    const { roll, pitch } = attitude.tilt();
    return Math.abs(roll) < 0.17 && Math.abs(pitch) < 0.17 && attitude.angularSpeed < 0.15;
  };
  const held = useHeld(flatAndStill, phase === "settle");

  const start = async () => {
    if (started.current) return;
    started.current = true;
    setBusy(true);
    try {
      await backend.startCalibration("gyro");
      setPhase("running");
      speak("Shh… stay perfectly still.");
    } catch (error) {
      started.current = false;
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (phase === "settle" && held > 1.5) void start();
  }, [held, phase]);

  useEffect(() => {
    if (phase !== "running" || !calibration) return;
    if (calibration.phase === "completed") setPhase("done");
    else if (["failed", "stopped", "unknown"].includes(calibration.phase)) {
      setFailure(calibration.message);
      setPhase("failed");
    }
  }, [calibration?.phase, phase]);

  const restart = () => {
    started.current = false;
    setReached([]);
    setPhase("wake");
  };

  if (phase === "orient") return <OrientationSetup sensorName="Gyroscope" onConfirm={() => setPhase("wake")} />;
  if (phase === "done") return <DoneCard step="gyro" title="Gyro is wide awake!" onRedo={restart} />;
  if (phase === "failed") return <FailCard message={failure} onRetry={() => (started.current = false, setPhase("settle"))} />;

  const running = phase === "running";
  const percent = calibration?.percent ?? null;

  return (
    <>
      <SceneTunnel.In>
        {phase === "wake" && (
          <>
            <ArmTrails />
            {ORBS.map((orb) => (
              <TargetOrb key={orb.id} color={orb.color} reached={reached.includes(orb.id)} position={orb.position as unknown as THREE.Vector3Tuple} />
            ))}
            {nextOrb && <SpinArrow axis={nextOrb.axis} radius={0.75} color={nextOrb.color} arc={Math.PI * 0.6} />}
          </>
        )}
        {(phase === "settle" || running) && (
          <StillnessRing progress={running ? (percent !== null ? percent / 100 : null) : Math.min(1, held / 1.5)} />
        )}
      </SceneTunnel.In>

      {phase === "wake" && (
        <div className="orb-dots">
          {ORBS.map((orb) => (
            <span key={orb.id} className={clsx("orb-dot", reached.includes(orb.id) && "is-hit")} style={{ "--dot": orb.color } as React.CSSProperties} />
          ))}
        </div>
      )}
      {running && (
        <div className="center-ring">
          <ProgressRing value={percent !== null ? percent / 100 : null} color={palette.triangle} />
        </div>
      )}

      <ActionBar>
        {phase === "wake" && (
          <PadButton glyph="square" onPress={() => setPhase("settle")}>
            Skip warm-up
          </PadButton>
        )}
        {phase === "settle" && (
          <PadButton glyph="cross" primary busy={busy} onPress={start}>
            It's still — go!
          </PadButton>
        )}
        {running && ACTIVE_PHASES.includes(calibration?.phase ?? "") && (
          <PadButton glyph="circle" onPress={() => backend.stopCalibration().catch(showError)}>
            Cancel
          </PadButton>
        )}
      </ActionBar>
    </>
  );
}
