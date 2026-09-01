import { JSX, useRef } from "react";
import { Canvas, useFrame } from "@react-three/fiber";
import * as THREE from "three";
import { useGyro } from "./useGyro";

function PhoneModel() {
  const meshRef = useRef<THREE.Mesh>(null);
  const gyro = useGyro();
  useFrame((_, delta) => {
    if (meshRef.current) {
      meshRef.current.quaternion.slerp(gyro.quaternion.current, 10 * delta);
    }
  });

  return (
    <mesh ref={meshRef}>
      <boxGeometry args={[2, 4, 0.4]} />
      <meshStandardMaterial color="coral" />
    </mesh>
  );
}

export function GyroReceiverPage(): JSX.Element {
  return (
    <div className="w-screen h-screen bg-[#111] flex flex-col">
      <h1 className="text-white m-0">360°</h1>
      <Canvas camera={{ position: [0, 0, 8], fov: 50 }}>
        <ambientLight intensity={1} />
        <directionalLight position={[5, 5, 5]} intensity={2} />
        <PhoneModel />
      </Canvas>
    </div>
  );
}
