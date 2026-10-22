import { RoundedBox } from "@react-three/drei";
import { type ThreeEvent, useFrame } from "@react-three/fiber";
import { type JSX, type MutableRefObject, useMemo, useRef } from "react";
import * as THREE from "three";
import { palette } from "../../theme";
import { useGhostMaterial } from "./Drone";

/** Normalised transmitter state: sticks/knobs -1..1, switches -1/0/1. */
export type TxState = {
  roll: number;
  pitch: number;
  throttle: number;
  yaw: number;
  switches: [number, number, number, number];
  knobs: [number, number];
  /** Raw channel values for the LCD (µs), channels 1-6. */
  channels: number[];
};

export function makeTxState(): TxState {
  return {
    roll: 0,
    pitch: 0,
    throttle: -1,
    yaw: 0,
    switches: [-1, -1, -1, -1],
    knobs: [-1, -1],
    channels: [1500, 1500, 1000, 1500, 1000, 1000],
  };
}

export type StickGhost = { x?: number; y?: number } | null;

const MAX_TILT = THREE.MathUtils.degToRad(26);

type Hand = "left" | "right";

function Gimbal({
  hand,
  read,
  ghost,
  highlight,
  onDrag,
  ...props
}: JSX.IntrinsicElements["group"] & {
  hand: Hand;
  read: () => { x: number; y: number };
  ghost: StickGhost;
  highlight: boolean;
  onDrag?: (hand: Hand, dx: number, dy: number, done: boolean) => void;
}) {
  const stick = useRef<THREE.Group>(null);
  const ghostStick = useRef<THREE.Group>(null);
  const ring = useRef<THREE.MeshStandardMaterial>(null);
  const ghostMaterial = useGhostMaterial(palette.gold, 1);
  const dragging = useRef(false);

  useFrame(({ clock }, dt) => {
    const { x, y } = read();
    if (stick.current) {
      stick.current.rotation.x = THREE.MathUtils.damp(stick.current.rotation.x, -y * MAX_TILT, 18, dt);
      stick.current.rotation.y = THREE.MathUtils.damp(stick.current.rotation.y, x * MAX_TILT, 18, dt);
    }
    if (ghostStick.current && ghost) {
      ghostStick.current.rotation.x = THREE.MathUtils.damp(ghostStick.current.rotation.x, -(ghost.y ?? y) * MAX_TILT, 6, dt);
      ghostStick.current.rotation.y = THREE.MathUtils.damp(ghostStick.current.rotation.y, (ghost.x ?? x) * MAX_TILT, 6, dt);
    }
    if (ring.current) {
      ring.current.emissiveIntensity = highlight ? 1.2 + Math.sin(clock.elapsedTime * 5) * 0.6 : 0.15;
    }
  });

  const down = (e: ThreeEvent<PointerEvent>) => {
    if (!onDrag) return;
    e.stopPropagation();
    dragging.current = true;
    (e.target as Element).setPointerCapture?.(e.pointerId);
  };
  const move = (e: ThreeEvent<PointerEvent>) => {
    if (!dragging.current || !onDrag) return;
    e.stopPropagation();
    onDrag(hand, e.nativeEvent.movementX / 90, -e.nativeEvent.movementY / 90, false);
  };
  const up = (e: ThreeEvent<PointerEvent>) => {
    if (!dragging.current || !onDrag) return;
    dragging.current = false;
    (e.target as Element).releasePointerCapture?.(e.pointerId);
    onDrag(hand, 0, 0, true);
  };

  const stickGeometry = (
    <>
      <mesh position={[0, 0, 0.12]} rotation={[Math.PI / 2, 0, 0]}>
        <cylinderGeometry args={[0.022, 0.03, 0.24, 16]} />
        <meshStandardMaterial color="#c9d2e3" metalness={0.7} roughness={0.25} />
      </mesh>
      <mesh position={[0, 0, 0.26]} rotation={[Math.PI / 2, 0, 0]}>
        <cylinderGeometry args={[0.055, 0.05, 0.08, 24]} />
        <meshStandardMaterial color={palette.graphite} roughness={0.6} />
      </mesh>
    </>
  );

  return (
    <group {...props}>
      <mesh rotation={[Math.PI / 2, 0, 0]}>
        <cylinderGeometry args={[0.2, 0.2, 0.02, 48]} />
        <meshStandardMaterial color={palette.ink} roughness={0.7} />
      </mesh>
      <mesh position={[0, 0, 0.012]}>
        <torusGeometry args={[0.2, 0.014, 12, 64]} />
        <meshStandardMaterial ref={ring} color={palette.cyan} emissive={palette.cyan} toneMapped={false} />
      </mesh>
      <mesh position={[0, 0, 0.02]}>
        <sphereGeometry args={[0.07, 24, 16, 0, Math.PI * 2, 0, Math.PI / 2]} />
        <meshStandardMaterial color={palette.graphite} roughness={0.4} />
      </mesh>
      <group
        ref={stick}
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
        onPointerCancel={up}
        onPointerOver={() => onDrag && (document.body.style.cursor = "grab")}
        onPointerOut={() => (document.body.style.cursor = "")}
      >
        {stickGeometry}
      </group>
      {ghost && (
        <group ref={ghostStick}>
          <mesh position={[0, 0, 0.14]} rotation={[Math.PI / 2, 0, 0]} material={ghostMaterial}>
            <cylinderGeometry args={[0.03, 0.03, 0.28, 16]} />
          </mesh>
          <mesh position={[0, 0, 0.27]} rotation={[Math.PI / 2, 0, 0]} material={ghostMaterial}>
            <cylinderGeometry args={[0.065, 0.06, 0.09, 24]} />
          </mesh>
        </group>
      )}
    </group>
  );
}

function Toggle({
  value,
  highlight,
  onClick,
  ...props
}: JSX.IntrinsicElements["group"] & { value: () => number; highlight: boolean; onClick?: () => void }) {
  const lever = useRef<THREE.Group>(null);
  const cap = useRef<THREE.MeshStandardMaterial>(null);
  useFrame(({ clock }, dt) => {
    if (lever.current) lever.current.rotation.x = THREE.MathUtils.damp(lever.current.rotation.x, value() * 0.55, 14, dt);
    if (cap.current) cap.current.emissiveIntensity = highlight ? 1 + Math.sin(clock.elapsedTime * 6) * 0.6 : 0;
  });
  return (
    <group {...props} onClick={(e) => (e.stopPropagation(), onClick?.())}>
      <mesh>
        <cylinderGeometry args={[0.04, 0.045, 0.03, 20]} />
        <meshStandardMaterial color="#c9d2e3" metalness={0.7} roughness={0.25} />
      </mesh>
      <group ref={lever}>
        <mesh position={[0, 0.08, 0]}>
          <cylinderGeometry args={[0.013, 0.016, 0.16, 12]} />
          <meshStandardMaterial color="#d8deea" metalness={0.8} roughness={0.2} />
        </mesh>
        <mesh position={[0, 0.17, 0]}>
          <sphereGeometry args={[0.024, 16, 16]} />
          <meshStandardMaterial ref={cap} color={palette.white} emissive={palette.gold} toneMapped={false} />
        </mesh>
      </group>
    </group>
  );
}

function Knob({ value, highlight, ...props }: JSX.IntrinsicElements["group"] & { value: () => number; highlight: boolean }) {
  const knob = useRef<THREE.Group>(null);
  const glow = useRef<THREE.MeshStandardMaterial>(null);
  useFrame(({ clock }, dt) => {
    if (knob.current) knob.current.rotation.y = THREE.MathUtils.damp(knob.current.rotation.y, -value() * 2.3, 12, dt);
    if (glow.current) glow.current.emissiveIntensity = highlight ? 1 + Math.sin(clock.elapsedTime * 6) * 0.6 : 0.2;
  });
  return (
    <group {...props}>
      <group ref={knob}>
        <mesh>
          <cylinderGeometry args={[0.05, 0.055, 0.05, 24]} />
          <meshStandardMaterial color={palette.graphite} roughness={0.5} />
        </mesh>
        <mesh position={[0, 0.027, 0.03]}>
          <boxGeometry args={[0.012, 0.006, 0.04]} />
          <meshStandardMaterial ref={glow} color={palette.square} emissive={palette.square} toneMapped={false} />
        </mesh>
      </group>
    </group>
  );
}

/** Live LCD: channel bars, like the FS-i6 "Display" screen. */
function Lcd({ state, ...props }: JSX.IntrinsicElements["mesh"] & { state: MutableRefObject<TxState> }) {
  const { canvas, texture } = useMemo(() => {
    const canvas = document.createElement("canvas");
    canvas.width = 256;
    canvas.height = 128;
    const texture = new THREE.CanvasTexture(canvas);
    texture.colorSpace = THREE.SRGBColorSpace;
    return { canvas, texture };
  }, []);
  const colors = [palette.triangle, palette.circle, palette.cross, palette.square, palette.gold, palette.cyan];
  useFrame(() => {
    const g = canvas.getContext("2d");
    if (!g) return;
    g.fillStyle = "#bfefff";
    g.fillRect(0, 0, 256, 128);
    g.fillStyle = palette.ink;
    g.font = "bold 16px Nunito Variable, sans-serif";
    g.fillText("FS-i6", 10, 20);
    state.current.channels.slice(0, 6).forEach((value, i) => {
      const y = 30 + i * 15;
      const fill = Math.min(1, Math.max(0, (value - 1000) / 1000));
      g.fillStyle = "rgba(10,31,68,0.15)";
      g.fillRect(36, y, 210, 10);
      g.fillStyle = colors[i];
      g.fillRect(36, y, 210 * fill, 10);
      g.fillStyle = palette.ink;
      g.font = "bold 11px Nunito Variable, sans-serif";
      g.fillText(`CH${i + 1}`, 6, y + 9);
    });
    texture.needsUpdate = true;
  });
  return (
    <mesh {...props}>
      <planeGeometry args={[0.46, 0.23]} />
      <meshBasicMaterial map={texture} toneMapped={false} />
    </mesh>
  );
}

export type TransmitterProps = JSX.IntrinsicElements["group"] & {
  state: MutableRefObject<TxState>;
  ghostLeft?: StickGhost;
  ghostRight?: StickGhost;
  highlight?: "left" | "right" | "both" | "switches" | null;
  onStickDrag?: (hand: Hand, dx: number, dy: number, done: boolean) => void;
  onSwitch?: (index: number) => void;
};

/**
 * FlySky FS-i6 in a glossy-toy style. Mode 2: left stick = throttle (Y) /
 * yaw (X), right stick = pitch (Y) / roll (X). SwA/SwB on the left shoulder,
 * SwC (3-position)/SwD on the right, VrA/VrB knobs in the middle.
 */
export function Transmitter({
  state,
  ghostLeft = null,
  ghostRight = null,
  highlight = null,
  onStickDrag,
  onSwitch,
  ...props
}: TransmitterProps) {
  const read = {
    left: () => ({ x: state.current.yaw, y: state.current.throttle }),
    right: () => ({ x: state.current.roll, y: state.current.pitch }),
  };
  const switchesGlow = highlight === "switches";
  return (
    <group {...props}>
      <RoundedBox args={[1.6, 1.0, 0.28]} radius={0.12} smoothness={5} castShadow>
        <meshStandardMaterial color={palette.plastic} roughness={0.22} />
      </RoundedBox>
      {[-1, 1].map((side) => (
        <RoundedBox key={side} args={[0.44, 0.62, 0.34]} radius={0.15} smoothness={5} position={[side * 0.6, -0.52, -0.03]} castShadow>
          <meshStandardMaterial color={palette.graphite} roughness={0.5} />
        </RoundedBox>
      ))}
      <RoundedBox args={[1.42, 0.8, 0.03]} radius={0.06} smoothness={4} position={[0, 0.02, 0.135]}>
        <meshStandardMaterial color={palette.deep} roughness={0.35} />
      </RoundedBox>

      <Gimbal hand="left" position={[-0.44, 0.1, 0.16]} read={read.left} ghost={ghostLeft} highlight={highlight === "left" || highlight === "both"} onDrag={onStickDrag} />
      <Gimbal hand="right" position={[0.44, 0.1, 0.16]} read={read.right} ghost={ghostRight} highlight={highlight === "right" || highlight === "both"} onDrag={onStickDrag} />

      {/* Trim rockers */}
      {[
        [-0.44, -0.16, 0.24, 0.04],
        [0.44, -0.16, 0.24, 0.04],
        [-0.7, 0.1, 0.04, 0.24],
        [0.7, 0.1, 0.04, 0.24],
      ].map(([x, y, w, h], i) => (
        <RoundedBox key={i} args={[w, h, 0.03]} radius={0.012} position={[x, y, 0.16]}>
          <meshStandardMaterial color={palette.cross} roughness={0.3} />
        </RoundedBox>
      ))}

      <Lcd state={state} position={[0, -0.25, 0.155]} />
      {[-0.17, -0.06, 0.06, 0.17].map((x, i) => (
        <RoundedBox key={x} args={[0.07, 0.035, 0.03]} radius={0.012} position={[x, -0.42, 0.155]}>
          <meshStandardMaterial color={[palette.triangle, palette.circle, palette.cross, palette.square][i]} roughness={0.3} />
        </RoundedBox>
      ))}

      {[-0.62, -0.4, 0.4, 0.62].map((x, i) => (
        <Toggle
          key={x}
          position={[x, 0.5, 0.02]}
          value={() => state.current.switches[i]}
          highlight={switchesGlow}
          onClick={onSwitch ? () => onSwitch(i) : undefined}
        />
      ))}
      {[-0.17, 0.17].map((x, i) => (
        <Knob key={x} position={[x, 0.5, 0.04]} value={() => state.current.knobs[i]} highlight={switchesGlow} />
      ))}

      {/* Folding antenna */}
      <group position={[0, 0.5, -0.1]} rotation={[-0.35, 0, 0]}>
        <mesh position={[0, 0.25, 0]}>
          <capsuleGeometry args={[0.035, 0.42, 8, 16]} />
          <meshStandardMaterial color={palette.graphite} roughness={0.5} />
        </mesh>
      </group>
    </group>
  );
}

/** FlySky FS-iA6B receiver with its two diversity antennas. */
export function Receiver({ linked, ...props }: JSX.IntrinsicElements["group"] & { linked: boolean }) {
  const led = useRef<THREE.MeshStandardMaterial>(null);
  useFrame(({ clock }) => {
    if (!led.current) return;
    const on = linked || Math.floor(clock.elapsedTime * 4) % 2 === 0;
    led.current.emissiveIntensity = on ? 3 : 0.1;
    led.current.emissive.set(linked ? palette.triangle : palette.danger);
  });
  const antennas = useMemo(
    () =>
      [
        [new THREE.Vector3(-0.24, 0.03, -0.08), new THREE.Vector3(-0.42, 0.06, -0.15), new THREE.Vector3(-0.72, 0.18, -0.12)],
        [new THREE.Vector3(-0.24, 0.03, 0.08), new THREE.Vector3(-0.4, 0.05, 0.25), new THREE.Vector3(-0.5, 0.15, 0.55)],
      ].map((points) => new THREE.TubeGeometry(new THREE.CatmullRomCurve3(points), 32, 0.009, 8, false)),
    [],
  );
  return (
    <group {...props}>
      <RoundedBox args={[0.48, 0.15, 0.3]} radius={0.03} smoothness={4} castShadow>
        <meshStandardMaterial color={palette.graphite} roughness={0.45} />
      </RoundedBox>
      <RoundedBox args={[0.4, 0.01, 0.22]} radius={0.004} position={[-0.02, 0.077, 0]}>
        <meshStandardMaterial color={palette.cross} roughness={0.3} />
      </RoundedBox>
      {/* Servo pin header: 3 rows (signal / + / -). */}
      {Array.from({ length: 18 }, (_, i) => (
        <mesh key={i} position={[0.245, 0.04 - (i % 3) * 0.035, -0.11 + Math.floor(i / 3) * 0.044]}>
          <boxGeometry args={[0.03, 0.012, 0.012]} />
          <meshStandardMaterial color="#e5c76b" metalness={0.9} roughness={0.2} />
        </mesh>
      ))}
      <mesh position={[0.12, 0.085, -0.1]}>
        <sphereGeometry args={[0.018, 16, 16]} />
        <meshStandardMaterial ref={led} color="#330000" emissive={palette.danger} toneMapped={false} />
      </mesh>
      {antennas.map((geometry, i) => (
        <group key={i}>
          <mesh geometry={geometry}>
            <meshStandardMaterial color="#1b2338" roughness={0.6} />
          </mesh>
          <mesh position={geometry.parameters.path.getPoint(1)}>
            <sphereGeometry args={[0.016, 12, 12]} />
            <meshStandardMaterial color={palette.white} />
          </mesh>
        </group>
      ))}
    </group>
  );
}
