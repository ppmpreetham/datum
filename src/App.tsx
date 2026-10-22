import "@fontsource-variable/nunito";
import "./App.css";
import { useEffect } from "react";
import { app, useApp } from "./app/state";
import { Bubble, ErrorToast, SafetyBanner, StatusChip, Stepper } from "./hud/Hud";
import { attitude, phoneOrientationToNed } from "./lib/attitude";
import { isTauri } from "./lib/backend";
import Stage from "./scene/Stage";
import { AccelStep } from "./steps/AccelStep";
import { CompassStep } from "./steps/CompassStep";
import { ConnectStep } from "./steps/ConnectStep";
import { EscStep } from "./steps/EscStep";
import { GyroStep } from "./steps/GyroStep";
import { LevelStep } from "./steps/LevelStep";
import { RadioStep } from "./steps/RadioStep";

// Handy for poking at live state from devtools during development.
if (import.meta.env.DEV) Object.assign(window, { __datum: { app, attitude } });

const STEPS = {
  connect: ConnectStep,
  gyro: GyroStep,
  accel: AccelStep,
  compass: CompassStep,
  level: LevelStep,
  radio: RadioStep,
  esc: EscStep,
};

/** Vehicle status + attitude from whichever backend is active. */
function useBackendEvents() {
  const backend = useApp((s) => s.backend);
  useEffect(() => {
    void backend.status().then((vehicle) => app.set({ vehicle }));
    const offVehicle = backend.onVehicle((vehicle) => {
      app.set({ vehicle });
      if (vehicle.phase !== "connected" && app.get().step !== "connect") {
        const active = vehicle.calibration && ["starting", "in_progress", "awaiting_user"].includes(vehicle.calibration.phase);
        if (!active) app.set({ step: "connect" });
      }
    });
    const offAttitude = backend.onAttitude((s) => attitude.setEuler(s.roll, s.pitch, s.yaw, "vehicle"));
    return () => {
      offVehicle();
      offAttitude();
    };
  }, [backend]);

  // Simulator inside the desktop app: a phone on /sender can stand in for the drone.
  useEffect(() => {
    if (backend.kind !== "sim" || !isTauri()) return;
    const socket = new WebSocket(`ws://${window.location.hostname || "127.0.0.1"}:8000/ws/gyro`);
    socket.onmessage = ({ data }) => {
      try {
        const { alpha, beta, gamma } = JSON.parse(data);
        if (alpha === null || beta === null || gamma === null) return;
        attitude.set(phoneOrientationToNed(alpha, beta, gamma), "phone");
      } catch {
        // Ignore anything that isn't an orientation sample.
      }
    };
    return () => socket.close();
  }, [backend]);
}

export default function App() {
  useBackendEvents();
  const step = useApp((s) => s.step);
  const kind = useApp((s) => s.backend.kind);

  // The simulated drone starts every step sitting flat and facing forward.
  useEffect(() => {
    if (kind !== "sim") return;
    attitude.setEuler(0, 0, 0, "drag");
    attitude.recenter();
  }, [step, kind]);
  const Step = STEPS[step];

  return (
    <div className="app">
      <Stage />
      <div className="hud">
        <Stepper />
        <StatusChip />
        <SafetyBanner />
        <Step key={step} />
        <Bubble />
        <ErrorToast />
      </div>
    </div>
  );
}
