import { useGLTF } from "@react-three/drei";
import { useFrame } from "@react-three/fiber";
import { type JSX, type MutableRefObject, useMemo, useRef } from "react";
import * as THREE from "three";
import type { GLTF } from "three-stdlib";
import { palette } from "../../theme";

type GLTFResult = GLTF & {
  nodes: {
    MainBody_LowPolyDrone_0: THREE.Mesh;
    Wing1_LowPolyDrone_0: THREE.Mesh;
    Wing2_LowPolyDrone_0: THREE.Mesh;
    Wing3_LowPolyDrone_0: THREE.Mesh;
    Wing4_LowPolyDrone_0: THREE.Mesh;
  };
  materials: { LowPolyDrone: THREE.MeshStandardMaterial };
};

const MODEL = "/models/drone_low_poly.glb";

/**
 * Propellers in ArduPilot motor-test order (A-D = front-right, then
 * clockwise), with their quad-X spin direction (+1 = CCW seen from above).
 * The GLB's Wing nodes are listed with their original transforms.
 */
const ROTORS = [
  { node: "Wing4", position: [47.952, 20.279, 47.669], scale: 24.322, dir: 1 },
  { node: "Wing3", position: [-47.952, 20.279, 47.669], scale: 24.322, dir: -1 },
  { node: "Wing2", position: [-47.952, 20.279, -47.669], scale: 100, dir: 1 },
  { node: "Wing1", position: [47.952, 20.279, -47.669], scale: 100, dir: -1 },
] as const;

/** Rotor hub positions in the drone's own space (scene units). */
export const ROTOR_POSITIONS: [number, number, number][] = ROTORS.map(({ position: [x, y, z] }) => [
  z * 0.01,
  y * 0.01,
  -x * 0.01,
]);
export const ROTOR_DIRECTIONS = ROTORS.map((r) => r.dir);

const ghostVertex = /* glsl */ `
  varying vec3 vNormal;
  varying vec3 vView;
  void main() {
    vec4 mv = modelViewMatrix * vec4(position, 1.0);
    vNormal = normalize(normalMatrix * normal);
    vView = normalize(-mv.xyz);
    gl_Position = projectionMatrix * mv;
  }
`;
const ghostFragment = /* glsl */ `
  uniform vec3 uColor;
  uniform float uOpacity;
  uniform float uTime;
  varying vec3 vNormal;
  varying vec3 vView;
  void main() {
    // Interpolated normals can push |dot| past 1; pow() of a negative is NaN on real GPUs,
    // and bloom then smears that NaN across the whole frame (black screen).
    float facing = abs(dot(normalize(vNormal), normalize(vView)));
    float rim = pow(clamp(1.0 - facing, 0.0, 1.0), 1.8);
    float scan = 0.85 + 0.15 * sin(gl_FragCoord.y * 0.35 - uTime * 4.0);
    gl_FragColor = vec4(uColor * (0.35 + rim * 1.6), (0.12 + rim * 0.9) * uOpacity * scan);
  }
`;

export function useGhostMaterial(color: string, opacity: number) {
  const material = useMemo(
    () =>
      new THREE.ShaderMaterial({
        vertexShader: ghostVertex,
        fragmentShader: ghostFragment,
        uniforms: {
          uColor: { value: new THREE.Color(color) },
          uOpacity: { value: opacity },
          uTime: { value: 0 },
        },
        transparent: true,
        depthWrite: false,
        blending: THREE.AdditiveBlending,
        side: THREE.DoubleSide,
        toneMapped: false,
      }),
    [],
  );
  material.uniforms.uColor.value.set(color);
  material.uniforms.uOpacity.value = opacity;
  useFrame(({ clock }) => (material.uniforms.uTime.value = clock.elapsedTime));
  return material;
}

export type DroneProps = JSX.IntrinsicElements["group"] & {
  /** Each frame the drone eases toward this orientation. */
  target?: MutableRefObject<THREE.Quaternion>;
  /** Render as a holographic ghost in this colour. */
  ghost?: string;
  opacity?: number;
  /** Rotor speed (0..1) in A-D order, read every frame. */
  speeds?: MutableRefObject<number[]>;
  propsAttached?: boolean;
  highlightMotor?: number | null;
  /** How quickly to follow the target (higher = snappier). */
  follow?: number;
};

export function Drone({
  target,
  ghost,
  opacity = 1,
  speeds,
  propsAttached = true,
  highlightMotor = null,
  follow = 10,
  ...props
}: DroneProps) {
  const { nodes, materials } = useGLTF(MODEL) as unknown as GLTFResult;
  const root = useRef<THREE.Group>(null);
  const rotors = useRef<(THREE.Group | null)[]>([]);
  const detach = useRef(propsAttached ? 0 : 1);
  const ghostMaterial = useGhostMaterial(ghost ?? palette.glow, opacity);

  const solid = useMemo(() => {
    const material = materials.LowPolyDrone.clone();
    material.roughness = 0.35;
    material.metalness = 0.05;
    return material;
  }, [materials]);
  solid.transparent = opacity < 1;
  solid.opacity = opacity;
  const material = ghost ? ghostMaterial : solid;

  useFrame((_, dt) => {
    if (target && root.current) {
      root.current.quaternion.slerp(target.current, 1 - Math.exp(-follow * dt));
    }
    detach.current = THREE.MathUtils.damp(detach.current, propsAttached ? 0 : 1, 3, dt);
    rotors.current.forEach((rotor, i) => {
      if (!rotor) return;
      const speed = speeds?.current[i] ?? 0;
      rotor.rotation.z += ROTORS[i].dir * speed * 40 * Math.min(dt, 0.05);
      // Props pop off: float up, tumble and shrink away.
      const d = detach.current;
      const base = ROTORS[i].position;
      rotor.position.set(base[0] * (1 + d * 0.6), base[1] + d * 60, base[2] * (1 + d * 0.6));
      rotor.visible = d < 0.98;
      rotor.scale.setScalar(ROTORS[i].scale * (1 - d));
    });
  });

  return (
    <group ref={root} {...props} dispose={null}>
      <group rotation={[0, Math.PI / 2, 0]} scale={0.01}>
        <group rotation={[-Math.PI / 2, 0, 0]} scale={24.322}>
          <mesh geometry={nodes.MainBody_LowPolyDrone_0.geometry} material={material} castShadow={!ghost} />
        </group>
        {ROTORS.map((rotor, i) => (
          <group
            key={rotor.node}
            ref={(g) => void (rotors.current[i] = g)}
            position={rotor.position as unknown as THREE.Vector3Tuple}
            rotation={[-Math.PI / 2, 0, 0]}
            scale={rotor.scale}
          >
            <mesh
              geometry={nodes[`${rotor.node}_LowPolyDrone_0` as keyof GLTFResult["nodes"]].geometry}
              material={material}
            />
          </group>
        ))}
      </group>
      {!ghost && <NavLights />}
      {!ghost &&
        highlightMotor !== null &&
        ROTOR_POSITIONS[highlightMotor] && (
          <mesh position={ROTOR_POSITIONS[highlightMotor]} rotation={[-Math.PI / 2, 0, 0]}>
            <ringGeometry args={[0.2, 0.25, 48]} />
            <meshBasicMaterial color={palette.gold} toneMapped={false} side={THREE.DoubleSide} />
          </mesh>
        )}
    </group>
  );
}

/** Cyan nose / pink tail lights so "front" is always obvious. */
function NavLights() {
  const front = useRef<THREE.MeshStandardMaterial>(null);
  useFrame(({ clock }) => {
    if (front.current) front.current.emissiveIntensity = 2 + Math.sin(clock.elapsedTime * 4) * 0.8;
  });
  return (
    <group>
      {[-0.07, 0.07].map((x) => (
        <mesh key={`f${x}`} position={[x, 0.05, -0.2]}>
          <sphereGeometry args={[0.025, 16, 16]} />
          <meshStandardMaterial ref={front} color={palette.cyan} emissive={palette.cyan} emissiveIntensity={2} toneMapped={false} />
        </mesh>
      ))}
      <mesh position={[0, 0.05, 0.2]}>
        <sphereGeometry args={[0.022, 16, 16]} />
        <meshStandardMaterial color={palette.square} emissive={palette.square} emissiveIntensity={2} toneMapped={false} />
      </mesh>
    </group>
  );
}

useGLTF.preload(MODEL);
