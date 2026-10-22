import { Float, Sparkles } from "@react-three/drei";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { Bloom, EffectComposer, Vignette } from "@react-three/postprocessing";
import { type JSX, Suspense, useEffect, useMemo, useRef, useState } from "react";
import * as THREE from "three";
import { SceneTunnel, app, useApp } from "../app/state";
import { attitude } from "../lib/attitude";
import { palette, symbolColors } from "../theme";
import { PulseRing, SymbolMesh, type PsSymbol } from "./fx";
import { Drone } from "./models/Drone";

/** Eases the camera toward the target the current step asks for. */
function CameraRig() {
  const camera = useThree((s) => s.camera) as THREE.PerspectiveCamera;
  const look = useRef(new THREE.Vector3(...app.get().camera.target));
  const goalPosition = useMemo(() => new THREE.Vector3(), []);
  const goalTarget = useMemo(() => new THREE.Vector3(), []);
  const pointer = useRef(new THREE.Vector2());

  useEffect(() => {
    const move = (e: PointerEvent) =>
      pointer.current.set((e.clientX / window.innerWidth) * 2 - 1, (e.clientY / window.innerHeight) * 2 - 1);
    window.addEventListener("pointermove", move);
    return () => window.removeEventListener("pointermove", move);
  }, []);

  useFrame((_, dt) => {
    const { position, target, fov } = app.get().camera;
    // A gentle parallax drift with the pointer keeps the scene alive.
    goalPosition.set(...position).add(new THREE.Vector3(pointer.current.x * 0.12, -pointer.current.y * 0.08, 0));
    goalTarget.set(...target);
    const lambda = 2.6;
    camera.position.x = THREE.MathUtils.damp(camera.position.x, goalPosition.x, lambda, dt);
    camera.position.y = THREE.MathUtils.damp(camera.position.y, goalPosition.y, lambda, dt);
    camera.position.z = THREE.MathUtils.damp(camera.position.z, goalPosition.z, lambda, dt);
    look.current.x = THREE.MathUtils.damp(look.current.x, goalTarget.x, lambda * 1.2, dt);
    look.current.y = THREE.MathUtils.damp(look.current.y, goalTarget.y, lambda * 1.2, dt);
    look.current.z = THREE.MathUtils.damp(look.current.z, goalTarget.z, lambda * 1.2, dt);
    camera.lookAt(look.current);
    if (Math.abs(camera.fov - fov) > 0.01) {
      camera.fov = THREE.MathUtils.damp(camera.fov, fov, lambda, dt);
      camera.updateProjectionMatrix();
    }
  });
  return null;
}

const FLOATERS: { symbol: PsSymbol; position: [number, number, number]; scale: number }[] = [
  { symbol: "triangle", position: [-4.2, 1.6, -4], scale: 0.5 },
  { symbol: "circle", position: [3.4, 2.9, -6.5], scale: 0.55 },
  { symbol: "cross", position: [-5.4, -1.2, -6], scale: 0.45 },
  { symbol: "square", position: [5.2, -1.0, -4.5], scale: 0.5 },
  { symbol: "circle", position: [-9, 2.5, -7], scale: 0.4 },
  { symbol: "triangle", position: [10, 1.5, -6], scale: 0.45 },
  { symbol: "square", position: [-11, -0.8, -5], scale: 0.4 },
  { symbol: "cross", position: [11.5, -1.6, -7], scale: 0.4 },
];

/** Floating face-button shapes and drifting light motes. */
function Backdrop() {
  return (
    <group>
      {FLOATERS.map((f, i) => (
        <Float key={i} speed={1.2 + (i % 3) * 0.3} rotationIntensity={1.2} floatIntensity={1.4}>
          <SymbolMesh
            symbol={f.symbol}
            size={f.scale}
            color={symbolColors[["triangle", "circle", "cross", "square"].indexOf(f.symbol)]}
            emissive={0.9}
            position={f.position}
          />
        </Float>
      ))}
      <Sparkles count={140} scale={[26, 9, 12]} position={[0, 0.5, -3]} size={2.2} speed={0.25} opacity={0.7} color={palette.glow} />
      <Sparkles count={40} scale={[6, 3, 4]} size={4} speed={0.4} opacity={0.5} color={palette.white} />
    </group>
  );
}

/** Soft round shadow under the hero drone (cheap, and robust on any GPU). */
function BlobShadow() {
  const texture = useMemo(() => {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 128;
    const g = canvas.getContext("2d")!;
    const gradient = g.createRadialGradient(64, 64, 0, 64, 64, 64);
    gradient.addColorStop(0, "rgba(1,20,60,0.55)");
    gradient.addColorStop(1, "rgba(1,20,60,0)");
    g.fillStyle = gradient;
    g.fillRect(0, 0, 128, 128);
    return new THREE.CanvasTexture(canvas);
  }, []);
  const visible = useApp((s) => s.drone.visible);
  if (!visible) return null;
  return (
    <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, -0.545, 0]}>
      <planeGeometry args={[1.8, 1.8]} />
      <meshBasicMaterial map={texture} transparent depthWrite={false} />
    </mesh>
  );
}

/** The glowing disc the hero object stands on. */
export function Platform({ radius = 1.2, ...props }: JSX.IntrinsicElements["group"] & { radius?: number }) {
  return (
    <group {...props}>
      <mesh rotation={[-Math.PI / 2, 0, 0]} receiveShadow>
        <circleGeometry args={[radius, 64]} />
        <meshStandardMaterial color={palette.skyBottom} transparent opacity={0.35} roughness={0.4} />
      </mesh>
      <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0.002, 0]}>
        <ringGeometry args={[radius * 0.97, radius, 96]} />
        <meshBasicMaterial color={palette.glow} toneMapped={false} />
      </mesh>
      <PulseRing radius={radius} speed={0.25} position={[0, 0.004, 0]} />
    </group>
  );
}

/**
 * Simulator only: drag anywhere on empty canvas to turn the drone like a
 * trackball (horizontal = yaw, vertical = pitch toward the camera, shift = roll).
 */
function DragRotate({ enabled }: { enabled: boolean }) {
  const gl = useThree((s) => s.gl);
  const camera = useThree((s) => s.camera);
  useEffect(() => {
    if (!enabled) return;
    const el = gl.domElement;
    let active = false;
    const down = (e: PointerEvent) => {
      if (e.button !== 0) return;
      active = true;
      el.setPointerCapture(e.pointerId);
    };
    const up = (e: PointerEvent) => {
      active = false;
      if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
    };
    const move = (e: PointerEvent) => {
      if (!active) return;
      const q = attitude.displayQuat();
      const right = new THREE.Vector3(1, 0, 0).applyQuaternion(camera.quaternion);
      const up = new THREE.Vector3(0, 1, 0);
      const forward = new THREE.Vector3(0, 0, -1).applyQuaternion(camera.quaternion);
      const turn = new THREE.Quaternion();
      if (e.shiftKey) {
        turn.setFromAxisAngle(forward, -e.movementX * 0.012);
      } else {
        turn.setFromAxisAngle(up, e.movementX * 0.012);
        turn.premultiply(new THREE.Quaternion().setFromAxisAngle(right, e.movementY * 0.012));
      }
      attitude.setFromDisplay(turn.multiply(q), "drag");
    };
    el.addEventListener("pointerdown", down);
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
    return () => {
      el.removeEventListener("pointerdown", down);
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
    };
  }, [enabled, gl, camera]);
  return null;
}

/** Keeps a simulated drone "alive": feeds the attitude so it never goes stale. */
function SimHeartbeat() {
  useFrame(() => {
    if (app.get().backend.kind === "sim" && attitude.source !== "phone") {
      attitude.set(attitude.quat, attitude.source === "none" ? "drag" : attitude.source);
    }
  });
  return null;
}

/** The hero drone that stays on stage across steps. */
function HeroDrone() {
  const drone = useApp((s) => s.drone);
  const target = useRef(new THREE.Quaternion());
  const speeds = useRef([0, 0, 0, 0]);
  const level = useMemo(() => new THREE.Quaternion(), []);
  const hover = useRef<THREE.Group>(null);
  useFrame(({ clock }, dt) => {
    if (drone.pose === "live" && attitude.isLive(5000)) attitude.displayQuat(target.current);
    else target.current.copy(level);
    speeds.current = drone.motorSpeeds.map((s, i) =>
      THREE.MathUtils.damp(speeds.current[i], drone.spinProps ? Math.max(s, 0.6) : s, 4, dt),
    );
    if (hover.current) hover.current.position.y = drone.spinProps ? Math.sin(clock.elapsedTime * 1.6) * 0.04 + 0.08 : 0;
  });
  if (!drone.visible) return null;
  return (
    <group ref={hover}>
      <Drone
        target={target}
        speeds={speeds}
        opacity={drone.opacity}
        propsAttached={drone.propsAttached}
        highlightMotor={drone.highlightMotor}
      />
    </group>
  );
}

export default function Stage() {
  const [frameloop, setFrameloop] = useState<"always" | "never">("always");
  const draggable = useApp((s) => s.backend.kind === "sim" && s.drone.draggable);

  useEffect(() => {
    const onVisibility = () => setFrameloop(document.hidden ? "never" : "always");
    document.addEventListener("visibilitychange", onVisibility);
    return () => document.removeEventListener("visibilitychange", onVisibility);
  }, []);

  return (
    <Canvas
      className="stage"
      frameloop={frameloop}
      shadows
      dpr={[1, Math.min(2, window.devicePixelRatio)]}
      gl={{ powerPreference: "high-performance", alpha: true, antialias: true }}
      camera={{ position: app.get().camera.position, fov: 40, near: 0.05, far: 100 }}
      onCreated={({ gl }) => gl.setClearColor(0x000000, 0)}
    >
      <CameraRig />
      <DragRotate enabled={draggable} />
      <SimHeartbeat />
      <hemisphereLight args={[palette.white, palette.skyBottom, 1.1]} />
      <directionalLight position={[3, 6, 4]} intensity={2.2} castShadow shadow-mapSize={[1024, 1024]} />
      <directionalLight position={[-4, 2, -3]} intensity={0.8} color={palette.glow} />
      <Suspense fallback={null}>
        <Backdrop />
        <HeroDrone />
        <Platform position={[0, -0.55, 0]} />
        <BlobShadow />
        <SceneTunnel.Out />
      </Suspense>
      <EffectComposer>
        <Bloom luminanceThreshold={0.85} luminanceSmoothing={0.3} intensity={0.9} mipmapBlur />
        <Vignette eskil={false} offset={0.15} darkness={0.85} />
      </EffectComposer>
    </Canvas>
  );
}
