import clsx from "clsx";
import { useEffect, useState } from "react";
import { CALIBRATION_STEPS, app, goTo, speak, useApp } from "../app/state";
import { setMuted, setVoice, sfx } from "../lib/sfx";
import { palette } from "../theme";

/**
 * Top stepper. Every calibration is reachable at any time once a vehicle (or
 * the simulator) is connected; completed ones get a check.
 */
export function Stepper() {
  const step = useApp((s) => s.step);
  const completed = useApp((s) => s.completed);
  const connected = useApp((s) => s.vehicle.phase === "connected");
  const [nudge, setNudge] = useState(0);
  const activeIndex = CALIBRATION_STEPS.findIndex((s) => s.id === step);

  const open = (id: (typeof CALIBRATION_STEPS)[number]["id"]) => {
    if (!connected) {
      sfx.error();
      setNudge((n) => n + 1);
      speak("Let's connect your drone first!", "warn");
      return;
    }
    if (app.get().vehicle.calibration && ["starting", "in_progress", "awaiting_user"].includes(app.get().vehicle.calibration!.phase)) {
      sfx.error();
      speak("Finish or cancel this one first.", "warn");
      return;
    }
    sfx.whoosh();
    goTo(id);
  };

  return (
    <nav className="stepper" key={nudge} data-shake={nudge > 0 || undefined}>
      <button
        type="button"
        className={clsx("stepper__node stepper__home", step === "connect" && "is-active")}
        onClick={() => (sfx.whoosh(), goTo("connect"))}
        title="Connection"
      >
        <PlugIcon />
      </button>
      <div className="stepper__track">
        <div
          className="stepper__fill"
          style={{ width: `${activeIndex < 0 ? 0 : (activeIndex / (CALIBRATION_STEPS.length - 1)) * 100}%` }}
        />
        {CALIBRATION_STEPS.map((s, i) => {
          const active = s.id === step;
          const done = completed[s.id];
          return (
            <button
              key={s.id}
              type="button"
              className={clsx("stepper__node", active && "is-active", done && "is-done", !connected && "is-locked")}
              style={{ "--node": s.color, left: `${(i / (CALIBRATION_STEPS.length - 1)) * 100}%` } as React.CSSProperties}
              onClick={() => open(s.id)}
            >
              <span className="stepper__glyph">{done ? "✓" : s.glyph}</span>
              <span className="stepper__label">{s.title}</span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}

function PlugIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round">
      <path d="M9 2v5M15 2v5M6 7h12v4a6 6 0 0 1-12 0V7zM12 17v5" />
    </svg>
  );
}

/** The talking speech bubble. Re-pops on every new line. */
export function Bubble() {
  const caption = useApp((s) => s.caption);
  if (!caption.text) return null;
  return (
    <div className="bubble-wrap">
      <div key={caption.id} className={clsx("bubble", `bubble--${caption.tone}`)}>
        <span className="bubble__dot" />
        {caption.text}
      </div>
    </div>
  );
}

export function StatusChip() {
  const vehicle = useApp((s) => s.vehicle);
  const kind = useApp((s) => s.backend.kind);
  const muted = useApp((s) => s.muted);
  const voice = useApp((s) => s.voice);
  const firmware = vehicle.firmware === "px4" ? "PX4" : vehicle.firmware === "ardu_pilot" ? "ArduPilot" : null;
  const color =
    vehicle.phase === "connected" ? palette.triangle : vehicle.phase === "connecting" ? palette.gold : palette.circle;
  return (
    <div className="status">
      <div className="status__chip">
        <span className="status__dot" style={{ background: color, boxShadow: `0 0 10px ${color}` }} />
        {kind === "sim" ? "Simulator" : firmware ?? (vehicle.phase === "connecting" ? "Connecting…" : "Offline")}
        {kind === "live" && vehicle.address && <span className="status__sub">{vehicle.address.split(":")[1]}</span>}
        {vehicle.armed && <span className="status__armed">ARMED</span>}
      </div>
      <button
        type="button"
        className={clsx("status__toggle", !voice && "is-off")}
        title={voice ? "Voice on" : "Voice off"}
        onClick={() => {
          setVoice(!voice);
          app.set({ voice: !voice });
        }}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round">
          <path d="M4 5h16v10H9l-5 4z" />
          {!voice && <path d="M3 3l18 18" />}
        </svg>
      </button>
      <button
        type="button"
        className={clsx("status__toggle", muted && "is-off")}
        title={muted ? "Sound off" : "Sound on"}
        onClick={() => {
          setMuted(!muted);
          app.set({ muted: !muted });
        }}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
          <path d="M4 9h4l5-4v14l-5-4H4z" />
          {muted ? <path d="M17 9l5 6M22 9l-5 6" /> : <path d="M17 8a5 5 0 0 1 0 8" />}
        </svg>
      </button>
    </div>
  );
}

export function ErrorToast() {
  const error = useApp((s) => s.error);
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    if (!error) return;
    sfx.error();
    setVisible(true);
    const timer = window.setTimeout(() => setVisible(false), 6500);
    return () => window.clearTimeout(timer);
  }, [error?.id]);
  if (!error || !visible) return null;
  return (
    <div className="toast" key={error.id} onClick={() => setVisible(false)}>
      <span className="toast__icon">!</span>
      {error.text}
    </div>
  );
}

/** A plain-language warning banner when the vehicle can't be calibrated now. */
export function SafetyBanner() {
  const vehicle = useApp((s) => s.vehicle);
  if (vehicle.phase !== "connected" || !vehicle.armed) return null;
  return <div className="banner">Your drone is armed. Disarm it before calibrating.</div>;
}
