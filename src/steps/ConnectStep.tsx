import clsx from "clsx";
import { useEffect, useState } from "react";
import { CALIBRATION_STEPS, app, goTo, showError, speak, useApp, useCamera, useDrone } from "../app/state";
import { ActionBar, PadButton, Panel, Title } from "../hud/Controls";
import { TauriBackend, isTauri } from "../lib/backend";
import { SimBackend } from "../lib/sim";
import { sfx } from "../lib/sfx";
import type { SerialPortSummary } from "../lib/types";

const BAUD_RATES = [57600, 115200, 230400, 460800, 921600];

export function ConnectStep() {
  const backend = useApp((s) => s.backend);
  const vehicle = useApp((s) => s.vehicle);
  const completed = useApp((s) => s.completed);
  const [ports, setPorts] = useState<SerialPortSummary[]>([]);
  const [port, setPort] = useState<string | null>(null);
  const [baud, setBaud] = useState(115200);
  const [advanced, setAdvanced] = useState(false);
  const [address, setAddress] = useState("udpin:127.0.0.1:14550");
  const [busy, setBusy] = useState(false);
  const connected = vehicle.phase === "connected";

  useDrone({ spinProps: true, pose: connected ? "live" : "level", draggable: connected });
  useCamera([0, 0.9, 3.4], [0, 0.05, 0]);

  const refresh = async () => {
    if (backend.kind === "sim") return;
    try {
      const found = await backend.listPorts();
      setPorts(found);
      setPort((current) => current ?? found.find((p) => p.portType === "usb")?.portName ?? found[0]?.portName ?? null);
    } catch (error) {
      showError(error);
    }
  };

  useEffect(() => {
    speak(connected ? "We're connected! Pick something to tune." : "Hi! Let's get your drone tuned up.");
    void refresh();
    const timer = window.setInterval(refresh, 3000);
    return () => window.clearInterval(timer);
  }, [backend]);

  useEffect(() => {
    if (vehicle.phase === "connected") {
      sfx.success();
      speak("Connected! Let's start with the gyro.", "good");
    } else if (vehicle.phase === "error" || vehicle.phase === "unsupported") {
      speak("Hmm, I couldn't talk to it.", "warn");
    }
  }, [vehicle.phase]);

  const connect = async () => {
    setBusy(true);
    try {
      if (backend.kind === "sim") await backend.connect("sim:datum:0");
      else if (advanced) await backend.connect(address.trim());
      else if (port) await backend.connectSerial(port, baud);
      else throw new Error("Plug in your flight controller with USB first.");
      speak("Listening for a heartbeat…");
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async () => {
    setBusy(true);
    try {
      await backend.disconnect();
      // Leaving the simulator inside the desktop app returns to real hardware.
      if (backend.kind === "sim" && isTauri()) app.set({ backend: new TauriBackend(), completed: {} });
    } finally {
      setBusy(false);
    }
  };

  const useSimulator = async () => {
    if (backend.kind === "live" && vehicle.phase !== "disconnected") await backend.disconnect();
    const sim = new SimBackend();
    app.set({ backend: sim, completed: {} });
    setBusy(true);
    try {
      await sim.connect("sim:datum:0");
      speak("Simulator on! Drag the drone to move it.", "good");
    } finally {
      setBusy(false);
    }
  };

  const firstOpen = CALIBRATION_STEPS.find((s) => !completed[s.id]) ?? CALIBRATION_STEPS[0];

  return (
    <>
      <Panel className="panel--left">
        <Title kicker="Datum">{connected ? "Ready to tune" : "Let's connect"}</Title>
        {connected ? (
          <p className="hint">
            {backend.kind === "sim"
              ? "You're on the simulator. Drag on the screen to tilt the drone; hold Shift to roll."
              : `${vehicle.firmware === "px4" ? "PX4" : "ArduPilot"} on ${vehicle.address}. Datum talks to it directly, so close QGroundControl's connection to this port first.`}
          </p>
        ) : backend.kind === "sim" ? (
          <p className="hint">No drone needed — everything runs on a pretend ArduPilot quad.</p>
        ) : advanced ? (
          <>
            <p className="hint">A MAVLink address, e.g. SITL or a telemetry bridge.</p>
            <input className="field" value={address} onChange={(e) => setAddress(e.target.value)} spellCheck={false} />
          </>
        ) : (
          <>
            <div className="chip-list">
              {ports.length === 0 && <div className="empty">Plug in your flight controller over USB…</div>}
              {ports.map((p) => (
                <button
                  key={p.portName}
                  type="button"
                  className={clsx("chip", port === p.portName && "is-selected")}
                  onClick={() => (sfx.pop(1), setPort(p.portName))}
                >
                  <strong>{p.portName}</strong>
                  <small>{p.product ?? p.manufacturer ?? p.portType}</small>
                </button>
              ))}
            </div>
            <div className="baud">
              {BAUD_RATES.map((rate) => (
                <button key={rate} type="button" className={clsx("baud__item", baud === rate && "is-selected")} onClick={() => setBaud(rate)}>
                  {rate / 1000}k
                </button>
              ))}
            </div>
          </>
        )}
        {!connected && backend.kind === "live" && (
          <button type="button" className="link" onClick={() => setAdvanced(!advanced)}>
            {advanced ? "Use a USB port" : "Use a network address"}
          </button>
        )}
        {vehicle.phase !== "disconnected" && !connected && <p className="hint hint--warn">{vehicle.message}</p>}
      </Panel>
      <ActionBar>
        {connected ? (
          <>
            <PadButton glyph="circle" onPress={disconnect} busy={busy}>
              Disconnect
            </PadButton>
            <PadButton glyph="cross" primary onPress={() => (sfx.whoosh(), goTo(firstOpen.id))}>
              Start {firstOpen.title}
            </PadButton>
          </>
        ) : (
          <>
            {isTauri() && backend.kind === "live" && (
              <PadButton glyph="square" onPress={useSimulator}>
                Try without a drone
              </PadButton>
            )}
            <PadButton glyph="cross" primary busy={busy || vehicle.phase === "connecting"} onPress={connect}>
              {backend.kind === "sim" ? "Start simulator" : "Connect"}
            </PadButton>
          </>
        )}
      </ActionBar>
    </>
  );
}
