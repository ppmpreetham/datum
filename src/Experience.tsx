import { Canvas } from "@react-three/fiber";
import { OrbitControls } from "@react-three/drei";

export default function Experience() {
  return (
    <Canvas className="h-full w-full bg-linear-to-b from-[#0099cc] to-[#024da1]">
      <mesh>
        <boxGeometry args={[2, 2, 2]} />
        <meshPhongMaterial />
      </mesh>
      <ambientLight intensity={0.1} />
      <directionalLight
        position={[0, 0, 5]}
        rotation={[0, Math.PI / 4, Math.PI / 4]}
        color="white"
      />
      <OrbitControls />
    </Canvas>
  );
}
