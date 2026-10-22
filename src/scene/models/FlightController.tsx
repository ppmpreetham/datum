import { RoundedBox } from "@react-three/drei";
import { useFrame } from "@react-three/fiber";
import { type JSX, useMemo, useRef } from "react";
import * as THREE from "three";
import { palette } from "../../theme";

/** Flat arrow shape pointing along -Z (the drone's forward). */
export function useArrowGeometry(length = 0.22, width = 0.14, depth = 0.012) {
  return useMemo(() => {
    const shape = new THREE.Shape();
    const shaft = width * 0.36;
    shape.moveTo(-shaft / 2, length / 2);
    shape.lineTo(shaft / 2, length / 2);
    shape.lineTo(shaft / 2, -length * 0.05);
    shape.lineTo(width / 2, -length * 0.05);
    shape.lineTo(0, -length / 2);
    shape.lineTo(-width / 2, -length * 0.05);
    shape.lineTo(-shaft / 2, -length * 0.05);
    shape.closePath();
    const geometry = new THREE.ExtrudeGeometry(shape, {
      depth,
      bevelEnabled: true,
      bevelSize: 0.006,
      bevelThickness: 0.006,
      bevelSegments: 2,
    });
    geometry.rotateX(Math.PI / 2);
    return geometry;
  }, [length, width, depth]);
}

/**
 * A toy-like autopilot board: glossy white case, gold forward arrow, and the
 * coloured servo rail / USB port so rotations are easy to read.
 */
export function FlightController(props: JSX.IntrinsicElements["group"]) {
  const arrow = useArrowGeometry();
  const led = useRef<THREE.MeshStandardMaterial>(null);
  useFrame(({ clock }) => {
    if (led.current) led.current.emissiveIntensity = 1.5 + Math.sin(clock.elapsedTime * 6) * 1.2;
  });
  return (
    <group {...props}>
      <RoundedBox args={[0.3, 0.07, 0.36]} radius={0.025} smoothness={4} castShadow>
        <meshStandardMaterial color={palette.plastic} roughness={0.25} />
      </RoundedBox>
      <RoundedBox args={[0.31, 0.02, 0.37]} radius={0.008} position={[0, -0.035, 0]}>
        <meshStandardMaterial color={palette.graphite} roughness={0.5} />
      </RoundedBox>
      <mesh geometry={arrow} position={[0, 0.048, 0]}>
        <meshStandardMaterial color={palette.gold} emissive={palette.gold} emissiveIntensity={0.6} roughness={0.3} />
      </mesh>
      {/* Servo rail at the back, USB at the side. */}
      {Array.from({ length: 8 }, (_, i) => (
        <mesh key={i} position={[-0.105 + i * 0.03, 0.01, 0.19]}>
          <boxGeometry args={[0.018, 0.04, 0.02]} />
          <meshStandardMaterial color={[palette.circle, palette.graphite, palette.white][i % 3]} />
        </mesh>
      ))}
      <mesh position={[0.158, 0, -0.05]}>
        <boxGeometry args={[0.02, 0.03, 0.06]} />
        <meshStandardMaterial color="#b9c3d6" metalness={0.8} roughness={0.2} />
      </mesh>
      <mesh position={[-0.1, 0.04, -0.13]}>
        <sphereGeometry args={[0.012, 12, 12]} />
        <meshStandardMaterial ref={led} color={palette.triangle} emissive={palette.triangle} toneMapped={false} />
      </mesh>
    </group>
  );
}

/** External GPS/compass puck on a short folding mast. */
export function GpsPuck({ mast = true, ...props }: JSX.IntrinsicElements["group"] & { mast?: boolean }) {
  const arrow = useArrowGeometry(0.12, 0.08, 0.008);
  return (
    <group {...props}>
      {mast && (
        <mesh position={[0, -0.18, 0]}>
          <cylinderGeometry args={[0.012, 0.012, 0.32, 12]} />
          <meshStandardMaterial color={palette.graphite} />
        </mesh>
      )}
      <mesh castShadow>
        <cylinderGeometry args={[0.12, 0.13, 0.05, 48]} />
        <meshStandardMaterial color={palette.plastic} roughness={0.2} />
      </mesh>
      <mesh position={[0, 0.026, 0]}>
        <cylinderGeometry args={[0.1, 0.1, 0.004, 48]} />
        <meshStandardMaterial color={palette.cross} roughness={0.4} />
      </mesh>
      <mesh geometry={arrow} position={[0, 0.034, 0]}>
        <meshStandardMaterial color={palette.gold} emissive={palette.gold} emissiveIntensity={0.6} />
      </mesh>
    </group>
  );
}
