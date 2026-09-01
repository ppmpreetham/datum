import * as THREE from "three";
import { Trail, useGLTF } from "@react-three/drei";
import { GLTF } from "three-stdlib";
import { JSX, useRef } from "react";
import { useFrame } from "@react-three/fiber";
import { WingRotationDirection } from "../../utils/WingRotator";
import { useGyro } from "../../utils/useGyro";

type GLTFResult = GLTF & {
  nodes: {
    MainBody_LowPolyDrone_0: THREE.Mesh;
    Wing1_LowPolyDrone_0: THREE.Mesh;
    Wing2_LowPolyDrone_0: THREE.Mesh;
    Wing3_LowPolyDrone_0: THREE.Mesh;
    Wing4_LowPolyDrone_0: THREE.Mesh;
  };
  materials: {
    LowPolyDrone: THREE.MeshStandardMaterial;
  };
};

export function DroneModel(props: JSX.IntrinsicElements["group"]) {
  const { nodes, materials } = useGLTF("/models/drone_low_poly.glb") as unknown as GLTFResult;
  const wing1Ref = useRef<THREE.Group>(null);
  const wing2Ref = useRef<THREE.Group>(null);
  const wing3Ref = useRef<THREE.Group>(null);
  const wing4Ref = useRef<THREE.Group>(null);

  // TODO: get the actual rotation from the gyro of the drone
  const fullMeshRef = useRef<THREE.Group>(null);
  const gyro = useGyro();
  useFrame((_, delta) => {
    if (fullMeshRef.current) {
      fullMeshRef.current.quaternion.slerp(gyro.quaternion.current, 10 * delta);
    }
  });

  // TODO: later get the actual speeds of this from the rotor
  useFrame((_, delta) => {
    const speed = 25 * delta;
    const cw = WingRotationDirection.Clockwise * speed;
    const ccw = WingRotationDirection.Counterclockwise * speed;

    if (wing1Ref.current) wing1Ref.current.rotation.z += cw;
    if (wing2Ref.current) wing2Ref.current.rotation.z += ccw;
    if (wing3Ref.current) wing3Ref.current.rotation.z += cw;
    if (wing4Ref.current) wing4Ref.current.rotation.z += ccw;
  });

  return (
    <group {...props} dispose={null} ref={fullMeshRef}>
      <Trail width={0.2} length={5} color={"hotpink"} decay={1}>
        <group rotation={[0, Math.PI / 2, 0]} scale={0.01}>
          <group name="MainBody" rotation={[-Math.PI / 2, 0, 0]} scale={24.322}>
            <mesh
              name="MainBody_LowPolyDrone_0"
              castShadow
              receiveShadow
              geometry={nodes.MainBody_LowPolyDrone_0.geometry}
              material={materials.LowPolyDrone}
            />
          </group>
          <group
            ref={wing1Ref}
            name="Wing1"
            position={[47.952, 20.279, -47.669]}
            rotation={[-Math.PI / 2, 0, 0]}
            scale={100}
          >
            <mesh
              name="Wing1_LowPolyDrone_0"
              castShadow
              receiveShadow
              geometry={nodes.Wing1_LowPolyDrone_0.geometry}
              material={materials.LowPolyDrone}
            />
          </group>
          <group
            ref={wing2Ref}
            name="Wing2"
            position={[-47.952, 20.279, -47.669]}
            rotation={[-Math.PI / 2, 0, 0]}
            scale={100}
          >
            <mesh
              name="Wing2_LowPolyDrone_0"
              castShadow
              receiveShadow
              geometry={nodes.Wing2_LowPolyDrone_0.geometry}
              material={materials.LowPolyDrone}
            />
          </group>
          <group
            ref={wing3Ref}
            name="Wing3"
            position={[-47.952, 20.279, 47.669]}
            rotation={[-Math.PI / 2, 0, 0]}
            scale={24.322}
          >
            <mesh
              name="Wing3_LowPolyDrone_0"
              castShadow
              receiveShadow
              geometry={nodes.Wing3_LowPolyDrone_0.geometry}
              material={materials.LowPolyDrone}
            />
          </group>
          <group
            ref={wing4Ref}
            name="Wing4"
            position={[47.952, 20.279, 47.669]}
            rotation={[-Math.PI / 2, 0, 0]}
            scale={24.322}
          >
            <mesh
              name="Wing4_LowPolyDrone_0"
              castShadow
              receiveShadow
              geometry={nodes.Wing4_LowPolyDrone_0.geometry}
              material={materials.LowPolyDrone}
            />
          </group>
        </group>
      </Trail>
    </group>
  );
}

useGLTF.preload("/models/drone_low_poly.glb");
