import { Trail } from "@react-three/drei";
import { useFrame } from "@react-three/fiber";
import { type ReactNode, useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { SceneTunnel, type StepId, markComplete, nextStep, speak, useApp } from "../app/state";
import { ActionBar, PadButton, Panel, Title } from "../hud/Controls";
import { attitude } from "../lib/attitude";
import { sfx } from "../lib/sfx";
import type { CalibrationKind, CalibrationProgress } from "../lib/types";
import { Confetti } from "../scene/fx";
import { ROTOR_POSITIONS } from "../scene/models/Drone";
import { symbolColors } from "../theme";

export const ACTIVE_PHASES = ["starting", "accepted", "in_progress", "awaiting_user", "stopping"];

/** The vehicle's progress for one calibration kind (null if another runs). */
export function useCalibration(kind: CalibrationKind): CalibrationProgress | null {
  return useApp((s) => (s.vehicle.calibration?.kind === kind ? s.vehicle.calibration : null));
}

/** Fires confetti + fanfare whenever `trigger` increments. */
export function Celebration({ trigger, position }: { trigger: number; position?: [number, number, number] }) {
  useEffect(() => {
    if (trigger) sfx.success();
  }, [trigger]);
  return (
    <SceneTunnel.In>
      <Confetti trigger={trigger} position={position} />
    </SceneTunnel.In>
  );
}

/**
 * Coloured ribbons streaming from each rotor while the drone moves, the same
 * motion trails the Playroom controller tutorial draws.
 */
export function ArmTrails({ length = 4 }: { length?: number }) {
  const group = useRef<THREE.Group>(null);
  const goal = useRef(new THREE.Quaternion());
  useFrame((_, dt) => {
    if (!group.current) return;
    attitude.displayQuat(goal.current);
    group.current.quaternion.slerp(goal.current, 1 - Math.exp(-10 * dt));
  });
  return (
    <group ref={group}>
      {ROTOR_POSITIONS.map((position, i) => (
        <Trail key={i} width={1.2} length={length} color={symbolColors[i]} attenuation={(t) => t * t} decay={1.2}>
          <mesh position={position}>
            <sphereGeometry args={[0.025, 8, 8]} />
            <meshBasicMaterial color={symbolColors[i]} toneMapped={false} />
          </mesh>
        </Trail>
      ))}
    </group>
  );
}

/** Seconds a condition has held continuously (resets to 0 when false). */
export function useHeld(condition: () => boolean, active = true) {
  const [held, setHeld] = useState(0);
  const check = useRef(condition);
  check.current = condition;
  useEffect(() => {
    if (!active) {
      setHeld(0);
      return;
    }
    let since: number | null = null;
    const timer = window.setInterval(() => {
      const now = performance.now();
      if (check.current()) {
        since ??= now;
        setHeld((now - since) / 1000);
      } else {
        since = null;
        setHeld(0);
      }
    }, 100);
    return () => window.clearInterval(timer);
  }, [active]);
  return held;
}

/** The "you did it" card shared by every step. */
export function DoneCard({
  step,
  title,
  children,
  onRedo,
}: {
  step: StepId;
  title: string;
  children?: ReactNode;
  onRedo: () => void;
}) {
  const [trigger, setTrigger] = useState(0);
  useEffect(() => {
    markComplete(step);
    setTrigger(1);
    speak(title, "good");
  }, []);
  return (
    <>
      <Celebration trigger={trigger} />
      <Panel className="panel--center">
        <div className="done-badge">✓</div>
        <Title>{title}</Title>
        {children}
      </Panel>
      <ActionBar>
        <PadButton glyph="circle" onPress={onRedo}>
          Do it again
        </PadButton>
        <PadButton glyph="cross" primary onPress={() => nextStep(step)}>
          Next
        </PadButton>
      </ActionBar>
    </>
  );
}

export function FailCard({ message, onRetry, onBack }: { message: string; onRetry: () => void; onBack?: () => void }) {
  useEffect(() => {
    sfx.error();
    speak("Hmm, that didn't take. Let's try again.", "warn");
  }, []);
  return (
    <>
      <Panel className="panel--center">
        <div className="done-badge done-badge--fail">!</div>
        <Title>Not quite</Title>
        <p className="hint">{message}</p>
      </Panel>
      <ActionBar>
        {onBack && (
          <PadButton glyph="circle" onPress={onBack}>
            Back
          </PadButton>
        )}
        <PadButton glyph="cross" primary onPress={onRetry}>
          Try again
        </PadButton>
      </ActionBar>
    </>
  );
}
