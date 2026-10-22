import { useFrame } from "@react-three/fiber";
import { type JSX, useEffect, useMemo, useRef } from "react";
import * as THREE from "three";
import { palette, symbolColors } from "../theme";

export type PsSymbol = "triangle" | "circle" | "cross" | "square";

/** PlayStation face-button outline, built from a low-segment torus. */
export function SymbolGeometry({ symbol, size = 0.2, tube = 0.035 }: { symbol: PsSymbol; size?: number; tube?: number }) {
  if (symbol === "cross") return <boxGeometry args={[size * 2, tube * 2, tube * 2]} />;
  const segments = symbol === "triangle" ? 3 : symbol === "square" ? 4 : 40;
  return <torusGeometry args={[size, tube, 8, segments]} />;
}

export function SymbolMesh({
  symbol,
  size = 0.2,
  color,
  emissive = 0.6,
  ...props
}: { symbol: PsSymbol; size?: number; color: string; emissive?: number } & JSX.IntrinsicElements["group"]) {
  const material = (
    <meshStandardMaterial color={color} emissive={color} emissiveIntensity={emissive} roughness={0.35} />
  );
  return (
    <group {...props}>
      {symbol === "cross" ? (
        <>
          <mesh rotation={[0, 0, Math.PI / 4]}>
            <boxGeometry args={[size * 2, size * 0.32, size * 0.32]} />
            {material}
          </mesh>
          <mesh rotation={[0, 0, -Math.PI / 4]}>
            <boxGeometry args={[size * 2, size * 0.32, size * 0.32]} />
            {material}
          </mesh>
        </>
      ) : (
        <mesh rotation={[0, 0, symbol === "square" ? Math.PI / 4 : symbol === "triangle" ? Math.PI / 2 : 0]}>
          <SymbolGeometry symbol={symbol} size={size} tube={size * 0.16} />
          {material}
        </mesh>
      )}
    </group>
  );
}

const CONFETTI = 90;

/** A burst of tiny face-button shapes. Fires whenever `trigger` changes. */
export function Confetti({ trigger, position = [0, 0, 0], spread = 1 }: { trigger: number; position?: [number, number, number]; spread?: number }) {
  const mesh = useRef<THREE.InstancedMesh>(null);
  const particles = useMemo(
    () =>
      Array.from({ length: CONFETTI }, () => ({
        p: new THREE.Vector3(),
        v: new THREE.Vector3(),
        r: new THREE.Euler(),
        w: new THREE.Vector3(),
        life: 0,
      })),
    [],
  );
  const dummy = useMemo(() => new THREE.Object3D(), []);

  useEffect(() => {
    if (!trigger || !mesh.current) return;
    particles.forEach((particle, i) => {
      particle.p.set(0, 0, 0);
      const dir = new THREE.Vector3().randomDirection();
      dir.y = Math.abs(dir.y) + 0.4;
      particle.v.copy(dir.normalize().multiplyScalar((2 + Math.random() * 2.5) * spread));
      particle.w.set(Math.random() * 10, Math.random() * 10, Math.random() * 10);
      particle.life = 1.6 + Math.random() * 0.8;
      mesh.current!.setColorAt(i, new THREE.Color(symbolColors[i % symbolColors.length]));
    });
    if (mesh.current.instanceColor) mesh.current.instanceColor.needsUpdate = true;
  }, [trigger, particles, spread]);

  useFrame((_, dt) => {
    if (!mesh.current) return;
    const step = Math.min(dt, 0.05);
    particles.forEach((particle, i) => {
      if (particle.life > 0) {
        particle.life -= step;
        particle.v.y -= 4.5 * step;
        particle.v.multiplyScalar(1 - step * 0.8);
        particle.p.addScaledVector(particle.v, step);
        particle.r.x += particle.w.x * step;
        particle.r.y += particle.w.y * step;
      }
      dummy.position.copy(particle.p);
      dummy.rotation.copy(particle.r);
      dummy.scale.setScalar(particle.life > 0 ? Math.min(1, particle.life) * 0.05 : 0);
      dummy.updateMatrix();
      mesh.current!.setMatrixAt(i, dummy.matrix);
    });
    mesh.current.instanceMatrix.needsUpdate = true;
  });

  return (
    <instancedMesh ref={mesh} args={[undefined, undefined, CONFETTI]} position={position} frustumCulled={false}>
      <torusGeometry args={[0.6, 0.22, 6, 4]} />
      <meshStandardMaterial emissiveIntensity={0.4} emissive={palette.white} roughness={0.3} />
    </instancedMesh>
  );
}

/**
 * An arc arrow that circles `axis` (scene space). The arc keeps sliding in the
 * direction of rotation so it reads as motion, not just a shape.
 */
export function SpinArrow({
  axis,
  radius = 0.9,
  color = palette.gold,
  opacity = 1,
  arc = Math.PI * 0.9,
  speed = 1.4,
  tube = 0.03,
}: {
  axis: THREE.Vector3;
  radius?: number;
  color?: string;
  opacity?: number;
  arc?: number;
  speed?: number;
  tube?: number;
}) {
  const group = useRef<THREE.Group>(null);
  const spinner = useRef<THREE.Group>(null);
  const geometry = useMemo(() => {
    const points = Array.from({ length: 40 }, (_, i) => {
      const a = (i / 39) * arc;
      return new THREE.Vector3(Math.cos(a) * radius, Math.sin(a) * radius, 0);
    });
    return new THREE.TubeGeometry(new THREE.CatmullRomCurve3(points), 64, tube, 8, false);
  }, [radius, arc, tube]);
  const headPosition = useMemo(() => new THREE.Vector3(Math.cos(arc) * radius, Math.sin(arc) * radius, 0), [arc, radius]);

  const target = useMemo(() => new THREE.Quaternion(), []);
  useFrame((_, dt) => {
    if (!group.current || !spinner.current) return;
    if (axis.lengthSq() > 1e-6) {
      target.setFromUnitVectors(new THREE.Vector3(0, 0, 1), axis.clone().normalize());
      group.current.quaternion.slerp(target, Math.min(1, dt * 6));
    }
    spinner.current.rotation.z = (spinner.current.rotation.z + dt * speed) % (Math.PI * 2);
  });

  return (
    <group ref={group}>
      <group ref={spinner}>
        <mesh geometry={geometry}>
          <meshBasicMaterial color={color} transparent opacity={opacity} toneMapped={false} depthWrite={false} />
        </mesh>
        <mesh position={headPosition} rotation={[0, 0, arc]}>
          <coneGeometry args={[tube * 3, tube * 7, 16]} />
          <meshBasicMaterial color={color} transparent opacity={opacity} toneMapped={false} depthWrite={false} />
        </mesh>
      </group>
    </group>
  );
}

/** Ripples outward and fades, forever. */
export function PulseRing({
  radius = 0.6,
  color = palette.glow,
  speed = 1,
  ...props
}: { radius?: number; color?: string; speed?: number } & JSX.IntrinsicElements["group"]) {
  const rings = useRef<THREE.Mesh[]>([]);
  useFrame(({ clock }) => {
    rings.current.forEach((ring, i) => {
      if (!ring) return;
      const t = (clock.elapsedTime * speed + i / 3) % 1;
      ring.scale.setScalar(0.6 + t * 0.8);
      (ring.material as THREE.MeshBasicMaterial).opacity = (1 - t) * 0.6;
    });
  });
  return (
    <group {...props}>
      {[0, 1, 2].map((i) => (
        <mesh key={i} ref={(m) => void (rings.current[i] = m!)} rotation={[-Math.PI / 2, 0, 0]}>
          <ringGeometry args={[radius * 0.94, radius, 64]} />
          <meshBasicMaterial color={color} transparent toneMapped={false} depthWrite={false} side={THREE.DoubleSide} />
        </mesh>
      ))}
    </group>
  );
}

/** A soft glowing target ball that bobs and pops when `reached`. */
export function TargetOrb({
  color,
  reached,
  ...props
}: { color: string; reached: boolean } & JSX.IntrinsicElements["group"]) {
  const mesh = useRef<THREE.Mesh>(null);
  const scale = useRef(0);
  useFrame(({ clock }, dt) => {
    if (!mesh.current) return;
    const goal = reached ? 0.0001 : 1 + Math.sin(clock.elapsedTime * 3) * 0.08;
    scale.current = THREE.MathUtils.damp(scale.current, goal, reached ? 10 : 5, dt);
    mesh.current.scale.setScalar(scale.current);
  });
  return (
    <group {...props}>
      <mesh ref={mesh}>
        <sphereGeometry args={[0.09, 24, 24]} />
        <meshStandardMaterial color={color} emissive={color} emissiveIntensity={1.6} toneMapped={false} />
      </mesh>
      {!reached && <PulseRing radius={0.16} color={color} speed={0.8} rotation={[Math.PI / 2, 0, 0]} />}
    </group>
  );
}

/** Damped number for smooth visual transitions. */
export function useDamped(target: number, lambda = 6) {
  const value = useRef(target);
  useFrame((_, dt) => {
    value.current = THREE.MathUtils.damp(value.current, target, lambda, dt);
  });
  return value;
}
