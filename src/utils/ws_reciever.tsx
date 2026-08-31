import { JSX, useEffect, useRef } from "react";
import { Canvas, useFrame } from "@react-three/fiber";
import * as THREE from "three";
import { GyroData } from "../phone";

function PhoneModel() {
  const meshRef = useRef<THREE.Mesh>(null);
  const targetQuaternion = useRef<THREE.Quaternion>(new THREE.Quaternion());
  const socketRef = useRef<WebSocket | null>(null);
  const initialAlphaRef = useRef<number | null>(null);

  useEffect(() => {
    socketRef.current = new WebSocket(`ws://${window.location.hostname}:8000/ws/gyro`);
    
    // Re-usable math objects to prevent garbage collection stutters
    const euler = new THREE.Euler();
    const q1 = new THREE.Quaternion(-Math.sqrt(0.5), 0, 0, Math.sqrt(0.5)); // Portrait correction

    socketRef.current.onmessage = (event: MessageEvent): void => {
      try {
        const data: GyroData = JSON.parse(event.data);
        
        if (initialAlphaRef.current === null && data.alpha !== null) {
          initialAlphaRef.current = data.alpha;
        }

        const rawAlpha = data.alpha ?? 0;
        const offsetAlpha = initialAlphaRef.current ?? 0;
        
        // Convert degrees to radians. Offset alpha so it starts facing forward.
        const alpha = THREE.MathUtils.degToRad(rawAlpha - offsetAlpha); // Z
        const beta = THREE.MathUtils.degToRad(data.beta ?? 0);   // X'
        const gamma = THREE.MathUtils.degToRad(data.gamma ?? 0); // Y''

        // Standard DeviceOrientation to Quaternion math (fixes gimbal lock perfectly)
        euler.set(beta, alpha, -gamma, 'YXZ');
        targetQuaternion.current.setFromEuler(euler);
        targetQuaternion.current.multiply(q1);

      } catch (err) {
        console.error("Failed to parse WebSocket message:", err);
      }
    };

    return () => {
      if (socketRef.current) socketRef.current.close();
    };
  }, []);

  // Smoothly interpolate current rotation to target rotation every frame
  useFrame((_, delta) => {
    if (meshRef.current) {
      meshRef.current.quaternion.slerp(targetQuaternion.current, 10 * delta);
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
    <div style={{ width: "100vw", height: "100vh", background: "#111", display: "flex", flexDirection: "column" }}>
      <h1 style={{ color: "white", textAlign: "center", padding: "20px", margin: 0, fontFamily: "sans-serif" }}>
        360° Gyroscope Visualizer
      </h1>
      <Canvas camera={{ position: [0, 0, 8], fov: 50 }}>
        <ambientLight intensity={1} />
        <directionalLight position={[5, 5, 5]} intensity={2} />
        <PhoneModel />
      </Canvas>
    </div>
  );
}
