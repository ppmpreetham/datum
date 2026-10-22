import { RoundedBox } from "@react-three/drei";
import { useFrame } from "@react-three/fiber";
import { type JSX, type MutableRefObject, useEffect, useMemo, useRef } from "react";
import * as THREE from "three";
import { palette } from "../../theme";

export type EscVisualState = {
  /** 0..1 throttle the ESC is seeing (drives the gauge). */
  throttle: number;
  /** 0..1 motor speed (drives the bell and prop). */
  motorSpeed: number;
  /** +1 CCW, -1 CW seen from above. */
  direction: 1 | -1;
};

const GAUGE_SEGMENTS = 64;

function wire(points: [number, number, number][], radius = 0.016) {
  return new THREE.TubeGeometry(
    new THREE.CatmullRomCurve3(points.map((p) => new THREE.Vector3(...p))),
    48,
    radius,
    8,
    false,
  );
}

/** A ripple that expands from the ESC each time `beepKey` changes. */
function BeepRipples({ beepKey, color }: { beepKey: number; color: string }) {
  const rings = useRef<THREE.Mesh[]>([]);
  const started = useRef<number[]>([]);
  useEffect(() => {
    if (!beepKey) return;
    const now = performance.now();
    started.current = [now, now + 160, now + 320];
  }, [beepKey]);
  useFrame(() => {
    const now = performance.now();
    rings.current.forEach((ring, i) => {
      if (!ring) return;
      const t = (now - (started.current[i] ?? -1e9)) / 900;
      const active = t >= 0 && t <= 1;
      ring.visible = active;
      if (!active) return;
      ring.scale.setScalar(0.3 + t * 1.6);
      (ring.material as THREE.MeshBasicMaterial).opacity = (1 - t) * 0.8;
    });
  });
  return (
    <group position={[0, 0.06, 0]}>
      {[0, 1, 2].map((i) => (
        <mesh key={i} ref={(m) => void (rings.current[i] = m!)} rotation={[-Math.PI / 2, 0, 0]} visible={false}>
          <ringGeometry args={[0.36, 0.4, 64]} />
          <meshBasicMaterial color={color} transparent toneMapped={false} depthWrite={false} side={THREE.DoubleSide} />
        </mesh>
      ))}
    </group>
  );
}

export type EscBenchProps = JSX.IntrinsicElements["group"] & {
  state: MutableRefObject<EscVisualState>;
  plugged: boolean;
  propOn: boolean;
  ledColor: string;
  ledBlink?: boolean;
  beepKey?: number;
};

/**
 * ESC on a bench: battery → XT60 → ESC (in translucent heat-shrink) → three
 * phase wires → brushless motor. Everything reacts to `state` every frame.
 */
export function EscBench({ state, plugged, propOn, ledColor, ledBlink = false, beepKey = 0, ...props }: EscBenchProps) {
  const bell = useRef<THREE.Group>(null);
  const prop = useRef<THREE.Group>(null);
  const plug = useRef<THREE.Group>(null);
  const gauge = useRef<THREE.Mesh>(null);
  const led = useRef<THREE.MeshStandardMaterial>(null);
  const propDetach = useRef(propOn ? 0 : 1);
  const plugGap = useRef(plugged ? 0 : 1);

  const wires = useMemo(
    () => ({
      phases: [palette.cross, palette.triangle, palette.square].map((color, i) => ({
        color,
        geometry: wire([
          [0.27, 0, -0.05 + i * 0.05],
          [0.45, -0.02, -0.06 + i * 0.06],
          [0.62, -0.06, -0.04 + i * 0.04],
        ]),
      })),
      power: [palette.danger, palette.ink].map((color, i) => ({
        color,
        geometry: wire(
          [
            [-0.27, 0, -0.03 + i * 0.06],
            [-0.4, -0.02, -0.02 + i * 0.04],
            [-0.52, 0, 0],
          ],
          0.024,
        ),
      })),
      battery: [palette.danger, palette.ink].map((color, i) => ({
        color,
        geometry: wire(
          [
            [-0.78, 0, 0],
            [-0.9, -0.02, -0.02 + i * 0.04],
            [-1.02, 0, -0.03 + i * 0.06],
          ],
          0.024,
        ),
      })),
      signal: wire([
        [-0.1, 0.04, -0.12],
        [-0.15, 0.08, -0.35],
        [-0.05, 0.02, -0.55],
      ], 0.01),
    }),
    [],
  );

  const gaugeGeometry = useMemo(() => new THREE.RingGeometry(0.2, 0.24, GAUGE_SEGMENTS, 1, Math.PI / 2, Math.PI * 2), []);
  const gaugeMaterial = useMemo(
    () => new THREE.MeshBasicMaterial({ color: palette.triangle, toneMapped: false, side: THREE.DoubleSide }),
    [],
  );
  const throttleColor = useMemo(() => new THREE.Color(), []);

  useFrame(({ clock }, dt) => {
    const { throttle, motorSpeed, direction } = state.current;
    if (bell.current) bell.current.rotation.y += direction * motorSpeed * 45 * Math.min(dt, 0.05);
    propDetach.current = THREE.MathUtils.damp(propDetach.current, propOn ? 0 : 1, 3, dt);
    if (prop.current) {
      const d = propDetach.current;
      prop.current.position.set(d * 0.3, 0.14 + d * 0.9, 0);
      prop.current.rotation.z = d * 2.2;
      prop.current.scale.setScalar(Math.max(0.0001, 1 - d));
    }
    plugGap.current = THREE.MathUtils.damp(plugGap.current, plugged ? 0 : 1, 5, dt);
    if (plug.current) plug.current.position.x = -0.09 * plugGap.current - 0.05 * plugGap.current;

    // Throttle gauge: an arc that fills (and warms from green to pink).
    if (gauge.current) {
      const fill = Math.max(0, Math.min(1, throttle));
      gaugeGeometry.setDrawRange(0, Math.round(fill * GAUGE_SEGMENTS) * 6);
      gaugeMaterial.color.copy(throttleColor.set(palette.triangle).lerp(new THREE.Color(palette.gold), fill));
    }
    if (led.current) {
      led.current.emissive.set(ledColor);
      const blinkOn = !ledBlink || Math.floor(clock.elapsedTime * 5) % 2 === 0;
      led.current.emissiveIntensity = plugged && blinkOn ? 3 : 0.05;
    }
  });

  return (
    <group {...props}>
      {/* ESC body in translucent heat-shrink */}
      <RoundedBox args={[0.5, 0.06, 0.2]} radius={0.02} smoothness={3}>
        <meshStandardMaterial color="#1f4f3a" roughness={0.6} />
      </RoundedBox>
      {Array.from({ length: 6 }, (_, i) => (
        <mesh key={i} position={[-0.17 + i * 0.07, 0.035, 0.03 * (i % 2 ? 1 : -1)]}>
          <boxGeometry args={[0.05, 0.015, 0.05]} />
          <meshStandardMaterial color={palette.graphite} roughness={0.4} />
        </mesh>
      ))}
      <RoundedBox args={[0.56, 0.11, 0.25]} radius={0.05} smoothness={4} castShadow>
        <meshPhysicalMaterial color={palette.cross} transparent opacity={0.55} roughness={0.15} clearcoat={1} depthWrite={false} />
      </RoundedBox>
      <mesh position={[0.15, 0.065, -0.07]}>
        <sphereGeometry args={[0.018, 16, 16]} />
        <meshStandardMaterial ref={led} color="#222" emissive={ledColor} toneMapped={false} />
      </mesh>
      <mesh ref={gauge} geometry={gaugeGeometry} material={gaugeMaterial} position={[0, 0.07, 0]} rotation={[-Math.PI / 2, 0, 0]} />
      <mesh position={[0, 0.069, 0]} rotation={[-Math.PI / 2, 0, 0]}>
        <ringGeometry args={[0.2, 0.24, 64]} />
        <meshBasicMaterial color={palette.white} transparent opacity={0.18} depthWrite={false} />
      </mesh>
      <BeepRipples beepKey={beepKey} color={palette.gold} />

      {wires.phases.map(({ color, geometry }) => (
        <mesh key={color} geometry={geometry}>
          <meshStandardMaterial color={color} roughness={0.4} />
        </mesh>
      ))}
      {wires.power.map(({ color, geometry }) => (
        <mesh key={color} geometry={geometry}>
          <meshStandardMaterial color={color} roughness={0.4} />
        </mesh>
      ))}
      <mesh geometry={wires.signal}>
        <meshStandardMaterial color={palette.white} roughness={0.4} />
      </mesh>

      {/* XT60: ESC half fixed, battery half slides away when unplugged */}
      <RoundedBox args={[0.1, 0.07, 0.11]} radius={0.015} position={[-0.56, 0, 0]}>
        <meshStandardMaterial color="#ffcf3a" roughness={0.35} />
      </RoundedBox>
      <group ref={plug}>
        <RoundedBox args={[0.12, 0.08, 0.12]} radius={0.015} position={[-0.68, 0, 0]}>
          <meshStandardMaterial color="#ffcf3a" roughness={0.35} />
        </RoundedBox>
        {wires.battery.map(({ color, geometry }) => (
          <mesh key={color} geometry={geometry}>
            <meshStandardMaterial color={color} roughness={0.4} />
          </mesh>
        ))}
        <RoundedBox args={[0.5, 0.2, 0.3]} radius={0.04} smoothness={4} position={[-1.28, 0, 0]} castShadow>
          <meshStandardMaterial color={palette.square} roughness={0.3} />
        </RoundedBox>
        <RoundedBox args={[0.3, 0.205, 0.31]} radius={0.02} position={[-1.28, 0, 0]}>
          <meshStandardMaterial color={palette.ink} roughness={0.5} />
        </RoundedBox>
      </group>

      {/* Brushless outrunner */}
      <group position={[0.78, -0.06, 0]}>
        <mesh>
          <cylinderGeometry args={[0.15, 0.15, 0.03, 32]} />
          <meshStandardMaterial color={palette.graphite} metalness={0.6} roughness={0.3} />
        </mesh>
        <group ref={bell}>
          <mesh position={[0, 0.08, 0]} castShadow>
            <cylinderGeometry args={[0.13, 0.13, 0.12, 32]} />
            <meshStandardMaterial color={palette.cyan} metalness={0.5} roughness={0.25} />
          </mesh>
          {Array.from({ length: 6 }, (_, i) => (
            <mesh key={i} position={[Math.cos((i / 6) * Math.PI * 2) * 0.13, 0.08, Math.sin((i / 6) * Math.PI * 2) * 0.13]} rotation={[0, -(i / 6) * Math.PI * 2, 0]}>
              <boxGeometry args={[0.012, 0.09, 0.04]} />
              <meshStandardMaterial color={palette.ink} />
            </mesh>
          ))}
          <mesh position={[0, 0.16, 0]}>
            <cylinderGeometry args={[0.03, 0.03, 0.06, 16]} />
            <meshStandardMaterial color="#c9d2e3" metalness={0.9} roughness={0.15} />
          </mesh>
          <group ref={prop}>
            {[0, Math.PI].map((angle) => (
              <mesh key={angle} rotation={[0.12, angle, 0]} position={[Math.cos(angle) * 0.3, 0, -Math.sin(angle) * 0.3]}>
                <boxGeometry args={[0.55, 0.012, 0.07]} />
                <meshStandardMaterial color={palette.white} roughness={0.3} />
              </mesh>
            ))}
          </group>
        </group>
      </group>
    </group>
  );
}
