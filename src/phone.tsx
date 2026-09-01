export interface GyroData {
  alpha: number | null;
  beta: number | null;
  gamma: number | null;
}

interface DeviceOrientationEventDeviceMotion {
  requestPermission?: () => Promise<"granted" | "denied">;
}

import { type JSX, useEffect, useRef } from "react";

export function GyroSender(): JSX.Element {
  const ws = useRef<WebSocket | null>(null);

  useEffect(() => {
    ws.current = new WebSocket(`ws://${window.location.hostname}:8000/ws/gyro`);

    const handleOrientation = (event: DeviceOrientationEvent): void => {
      if (!ws.current || ws.current.readyState !== WebSocket.OPEN) return;

      const payload: GyroData = {
        alpha: event.alpha,
        beta: event.beta,
        gamma: event.gamma,
      };

      ws.current.send(JSON.stringify(payload));
    };

    const startListening = (): void => {
      window.addEventListener("deviceorientation", handleOrientation);
    };

    const requestPermission = async (): Promise<void> => {
      const DeviceOrientationClass =
        DeviceOrientationEvent as unknown as DeviceOrientationEventDeviceMotion;

      if (typeof DeviceOrientationClass.requestPermission === "function") {
        try {
          const response = await DeviceOrientationClass.requestPermission();
          if (response === "granted") {
            startListening();
          }
        } catch (error) {
          console.error("Permission request rejected:", error);
        }
      } else {
        startListening();
      }
    };
    (window as any).requestGyroPermission = requestPermission;

    return () => {
      window.removeEventListener("deviceorientation", handleOrientation);
      if (ws.current) {
        ws.current.close();
      }
    };
  }, []);

  const handleButtonClick = (): void => {
    if ((window as any).requestGyroPermission) {
      (window as any).requestGyroPermission();
    }
  };

  return (
    <button onClick={handleButtonClick} type="button" className="border border-black p-2">
      Start Sharing Gyroscope
    </button>
  );
}
