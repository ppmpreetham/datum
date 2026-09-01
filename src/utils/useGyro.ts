import { useEffect, useRef } from "react";
import * as THREE from "three";
import { GyroData } from "../phone";

export function useGyro(url?: string) {
  const quaternion = useRef<THREE.Quaternion>(new THREE.Quaternion());
  const rawData = useRef<GyroData>({ alpha: null, beta: null, gamma: null });

  const socketRef = useRef<WebSocket | null>(null);
  const initialAlphaRef = useRef<number | null>(null);

  useEffect(() => {
    const wsUrl = url || `ws://${window.location.hostname}:8000/ws/gyro`;
    socketRef.current = new WebSocket(wsUrl);

    const euler = new THREE.Euler();
    const q1 = new THREE.Quaternion(-Math.sqrt(0.5), 0, 0, Math.sqrt(0.5));

    socketRef.current.onmessage = (event: MessageEvent): void => {
      try {
        const data: GyroData = JSON.parse(event.data);
        rawData.current = data;

        if (initialAlphaRef.current === null && data.alpha !== null) {
          initialAlphaRef.current = data.alpha;
        }

        const rawAlpha = data.alpha ?? 0;
        const offsetAlpha = initialAlphaRef.current ?? 0;

        const alpha = THREE.MathUtils.degToRad(rawAlpha - offsetAlpha); // Z
        const beta = THREE.MathUtils.degToRad(data.beta ?? 0); // X'
        const gamma = THREE.MathUtils.degToRad(data.gamma ?? 0); // Y''

        euler.set(beta, alpha, -gamma, "YXZ");
        quaternion.current.setFromEuler(euler);
        quaternion.current.multiply(q1);
      } catch (err) {
        console.error("Failed to parse WebSocket message:", err);
      }
    };

    return () => {
      if (socketRef.current) socketRef.current.close();
    };
  }, [url]);

  return { quaternion, rawData };
}
