import { Canvas } from "@react-three/fiber";
import { Bloom, EffectComposer, LensFlare, Noise, Vignette } from "@react-three/postprocessing";
import { useEffect, useState } from "react";
import { DroneModel } from "./components/models/drone";

const getDevicePixelRatio = () => {
  return Math.min(1, window.devicePixelRatio);
};

export default function Experience() {
  const dpr = getDevicePixelRatio();
  const [frameloop, setFrameloop] = useState<"always" | "never">("always");

  useEffect(() => {
    const handleVisibilityChange = () => setFrameloop(document.hidden ? "never" : "always");
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => document.removeEventListener("visibilitychange", handleVisibilityChange);
  }, []);

  return (
    <Canvas
      className="h-full w-full bg-linear-to-b from-[#0099cc] to-[#024da1]"
      linear
      frameloop={frameloop}
      gl={{
        powerPreference: "high-performance",
        alpha: true,
      }}
      performance={{ min: 0.5 }}
      dpr={dpr}
      shadows={true}
      eventPrefix="client"
      onCreated={({ gl }) => gl.setClearColor(0x000000, 0)}
      // camera={{ position: [1.5, 0, 0] }}
      camera={{ position: [0, 0.5, 1.5] }}
    >
      <EffectComposer>
        <Bloom luminanceThreshold={0.9} luminanceSmoothing={0.9} height={300} />
        <Noise opacity={0.02} />
        <Vignette eskil={false} offset={0.1} darkness={1.1} />
        <LensFlare opacity={0.5} />
      </EffectComposer>
      <DroneModel />

      <ambientLight intensity={1} />
      <directionalLight position={[5, 5, 5]} color="white" intensity={1.5} />
      {/*<OrbitControls />*/}
    </Canvas>
  );
}
