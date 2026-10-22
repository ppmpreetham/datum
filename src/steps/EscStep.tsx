import clsx from "clsx";
import { useEffect, useRef, useState } from "react";
import { SceneTunnel, markComplete, nextStep, showError, speak, useBackend, useCamera, useDrone } from "../app/state";
import { ActionBar, PadButton, Panel, Title } from "../hud/Controls";
import { sfx } from "../lib/sfx";
import { setSimChannel, simRadioInputs } from "../lib/sim";
import type { EscCapabilities, EscStatus, RadioStatus } from "../lib/types";
import { PulseRing } from "../scene/fx";
import { ROTOR_DIRECTIONS } from "../scene/models/Drone";
import { EscBench, type EscVisualState } from "../scene/models/Esc";
import { Transmitter, makeTxState } from "../scene/models/Radio";
import { palette } from "../theme";
import { Celebration } from "./common";

const BENCH: [number, number, number] = [-2.7, -0.3, 0.6];

/** Per backend step: the line we say, and how the bench should look. */
const STEP_SCENES: Record<string, { say: string; title: string; plugged: boolean; propOn: boolean; throttleGoal?: number; showTx?: boolean }> = {
  remove_propellers: { say: "Props off first. Pop them off!", title: "Props off", plugged: false, propOn: true },
  disconnect_battery: { say: "Unplug the flight battery.", title: "Unplug battery", plugged: true, propOn: false },
  raise_throttle: { say: "Throttle stick all the way up.", title: "Throttle up", plugged: false, propOn: false, throttleGoal: 1, showTx: true },
  power_and_confirm_tones: { say: "Plug the battery in. Listen for the beeps!", title: "Power up", plugged: false, propOn: false, throttleGoal: 1, showTx: true },
  lower_throttle: { say: "Throttle all the way down. Wait for the long beep.", title: "Throttle down", plugged: true, propOn: false, throttleGoal: 0, showTx: true },
};

/** What the bench shows *after* each step is confirmed. */
const AFTER_CONFIRM: Record<string, { plugged: boolean; propOn: boolean }> = {
  remove_propellers: { plugged: true, propOn: false },
  disconnect_battery: { plugged: false, propOn: false },
  raise_throttle: { plugged: false, propOn: false },
  power_and_confirm_tones: { plugged: true, propOn: false },
  lower_throttle: { plugged: true, propOn: false },
};

const MOTOR_LABELS = ["A", "B", "C", "D"];
/** Quad-X layout positions for the motor picker (front-right, then clockwise). */
const PICKER = [
  { x: 70, y: 18 },
  { x: 70, y: 70 },
  { x: 18, y: 70 },
  { x: 18, y: 18 },
];

type Phase = "check" | "safety" | "guided" | "test";

export function EscStep() {
  const backend = useBackend();
  const [phase, setPhase] = useState<Phase>("check");
  const [capabilities, setCapabilities] = useState<EscCapabilities | null>(null);
  const [status, setStatus] = useState<EscStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [plugged, setPlugged] = useState(true);
  const [propOn, setPropOn] = useState(true);
  const [beepKey, setBeepKey] = useState(0);
  const [celebrate, setCelebrate] = useState(0);
  const [propsConfirmed, setPropsConfirmed] = useState(false);
  const [motor, setMotor] = useState(0);
  const [throttle, setThrottle] = useState(8);
  const [duration, setDuration] = useState(2);
  const [spinning, setSpinning] = useState(false);
  const esc = useRef<EscVisualState>({ throttle: 0, motorSpeed: 0, direction: 1 });
  const tx = useRef(makeTxState());
  const rcThrottle = useRef<number | null>(null);

  const testing = phase === "test";
  useDrone({
    pose: "level",
    propsAttached: phase === "guided" ? propOn : !testing,
    motorSpeeds: [0, 1, 2, 3].map((i) => (spinning && i === motor ? throttle / 15 : 0)) as [number, number, number, number],
    highlightMotor: testing ? motor : null,
    draggable: false,
  });
  useCamera(
    testing ? [-1.7, 2.4, 3.7] : [BENCH[0] - 0.1, 1.0, BENCH[2] + 2.6],
    testing ? [-1.7, -0.25, 0.2] : [BENCH[0] - 0.1, BENCH[1] - 0.05, BENCH[2]],
  );

  // RC throttle (channel 3) drives the ESC gauge and the 3D stick, when available.
  useEffect(
    () =>
      backend.onRadio((radio: RadioStatus) => {
        const ch3 = radio.channels.find((c) => c.channel === 3);
        rcThrottle.current = ch3 ? (ch3.value_us - 1000) / 1000 : null;
      }),
    [backend],
  );

  useEffect(() => {
    let cancelled = false;
    speak("Let's check how your ESCs talk.");
    backend
      .escCapabilities()
      .then((caps) => {
        if (cancelled) return;
        setCapabilities(caps);
        if (!caps.endpoint_calibration_required) {
          speak(`${caps.protocol.toUpperCase()} is digital — no calibration needed! Let's test the motors.`, "good");
        } else {
          speak("Your ESCs use PWM. Let's teach them the throttle range.");
        }
      })
      .catch(showError);
    return () => {
      cancelled = true;
    };
  }, [backend]);

  const scene = status?.active ? STEP_SCENES[status.step_id] : null;

  useEffect(() => {
    if (!scene) return;
    speak(scene.say);
    setPlugged(scene.plugged);
    setPropOn(scene.propOn);
  }, [status?.step_id]);

  // Throttle on the bench follows the RC stick if we have it, else the step's goal.
  useEffect(() => {
    let raf = 0;
    const loop = () => {
      const goal = testing ? throttle / 100 : (rcThrottle.current ?? scene?.throttleGoal ?? 0);
      const s = esc.current;
      s.throttle += (Math.max(0, Math.min(1, goal)) - s.throttle) * 0.08;
      s.motorSpeed += ((testing && spinning ? throttle / 15 : 0) - s.motorSpeed) * 0.08;
      s.direction = ROTOR_DIRECTIONS[motor] as 1 | -1;
      tx.current.throttle = s.throttle * 2 - 1;
      tx.current.channels[2] = 1000 + s.throttle * 1000;
      raf = requestAnimationFrame(loop);
    };
    loop();
    return () => cancelAnimationFrame(raf);
  }, [testing, throttle, spinning, motor, scene]);

  const startGuided = async () => {
    setBusy(true);
    try {
      setStatus(await backend.startEsc());
      setPropsConfirmed(true);
      setPhase("guided");
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  const advance = async () => {
    if (!status) return;
    setBusy(true);
    try {
      const after = AFTER_CONFIRM[status.step_id];
      if (after) {
        setPlugged(after.plugged);
        setPropOn(after.propOn);
      }
      if (status.step_id === "power_and_confirm_tones") {
        // Classic PWM ESC: startup melody, then two beeps for "max throttle seen".
        sfx.escBeeps([[523, 120], [659, 120], [784, 160], [1046, 140], [1046, 140]]);
        setBeepKey((k) => k + 1);
      }
      if (status.step_id === "lower_throttle") {
        sfx.escBeeps([[784, 500]]);
        setBeepKey((k) => k + 1);
      }
      const next = await backend.advanceEsc(status.session_id, status.step_id);
      setStatus(next);
      if (!next.active) {
        markComplete("esc");
        setCelebrate((c) => c + 1);
        speak("ESCs know their range now! Let's spin some motors.", "good");
        setPhase("test");
      } else sfx.ding();
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  const cancelGuided = async () => {
    if (status) await backend.cancelEsc(status.session_id).catch(showError);
    setStatus(null);
    setPhase("check");
  };

  const spin = async () => {
    setBusy(true);
    try {
      const result = await backend.motorTest(motor + 1, throttle, duration);
      if (!result.accepted) throw new Error(result.message);
      sfx.whoosh();
      setSpinning(true);
      speak(`Motor ${MOTOR_LABELS[motor]} — spinning ${ROTOR_DIRECTIONS[motor] > 0 ? "counter-clockwise" : "clockwise"}?`);
      window.setTimeout(() => setSpinning(false), duration * 1000);
      markComplete("esc");
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  // Simulator: drag the throttle stick on the 3D transmitter.
  const onThrottleDrag = (hand: "left" | "right", _dx: number, dy: number) => {
    if (hand !== "left") return;
    setSimChannel(3, simRadioInputs[2] + dy * 500);
  };

  const ledColor = plugged ? (status?.step_id === "power_and_confirm_tones" ? palette.gold : palette.triangle) : palette.circle;

  return (
    <>
      <SceneTunnel.In>
        <group position={BENCH}>
          <EscBench
            state={esc}
            plugged={testing ? true : plugged}
            propOn={testing ? false : propOn}
            ledColor={ledColor}
            ledBlink={status?.step_id === "power_and_confirm_tones"}
            beepKey={beepKey}
          />
          <PulseRing radius={0.95} color={palette.glow} speed={0.2} position={[0, -0.12, 0]} />
        </group>
        {scene?.showTx && (
          <group position={[BENCH[0] + 0.15, BENCH[1] + 0.3, BENCH[2] - 1.0]} scale={0.5}>
            <Transmitter
              state={tx}
              ghostLeft={{ x: 0, y: (scene.throttleGoal ?? 0) * 2 - 1 }}
              highlight="left"
              onStickDrag={backend.kind === "sim" ? onThrottleDrag : undefined}
            />
          </group>
        )}
      </SceneTunnel.In>
      <Celebration trigger={celebrate} position={BENCH} />

      {phase === "check" && (
        <Panel className="panel--left">
          <Title kicker="ESC">{capabilities ? capabilities.protocol.replace("_", " ").toUpperCase() : "Checking…"}</Title>
          {capabilities && (
            <p className="hint">
              {capabilities.endpoint_calibration_required
                ? "Analog-style ESCs need to learn where throttle starts and ends."
                : "Digital protocol: throttle range is fixed, nothing to calibrate."}
            </p>
          )}
        </Panel>
      )}
      {phase === "safety" && (
        <Panel className="panel--left">
          <Title kicker="ESC · safety">Props off. Really.</Title>
          <p className="hint">Motors can spin at full power during this. Every propeller must be removed.</p>
          <label className="check">
            <input type="checkbox" checked={propsConfirmed} onChange={(e) => setPropsConfirmed(e.target.checked)} />
            All propellers are off
          </label>
        </Panel>
      )}
      {phase === "guided" && status && scene && (
        <div className="side-hud side-hud--left">
          <div className="esc-steps">
            {Object.entries(STEP_SCENES).map(([id, s]) => {
              const order = Object.keys(STEP_SCENES);
              const index = order.indexOf(id);
              const current = order.indexOf(status.step_id);
              return (
                <div key={id} className={clsx("esc-step", index < current && "is-done", index === current && "is-now")}>
                  <span>{index < current ? "✓" : index + 1}</span>
                  {s.title}
                </div>
              );
            })}
          </div>
        </div>
      )}
      {testing && (
        <Panel className="panel--left">
          <Title kicker="Motor test">Spin a motor</Title>
          <div className="motor-picker">
            <div className="motor-picker__frame">
              <span className="motor-picker__nose">▲</span>
              {PICKER.map((p, i) => (
                <button
                  key={i}
                  type="button"
                  className={clsx("motor-picker__motor", motor === i && "is-selected", spinning && motor === i && "is-spinning")}
                  style={{ left: `${p.x}%`, top: `${p.y}%` }}
                  onClick={() => (sfx.pop(i), setMotor(i))}
                >
                  {MOTOR_LABELS[i]}
                  <small>{ROTOR_DIRECTIONS[i] > 0 ? "↺" : "↻"}</small>
                </button>
              ))}
            </div>
          </div>
          <label className="slider">
            <span>Throttle</span>
            <input type="range" min={1} max={15} step={1} value={throttle} onChange={(e) => setThrottle(Number(e.target.value))} />
            <b>{throttle}%</b>
          </label>
          <label className="slider">
            <span>Time</span>
            <input type="range" min={0.5} max={5} step={0.5} value={duration} onChange={(e) => setDuration(Number(e.target.value))} />
            <b>{duration}s</b>
          </label>
          {!propsConfirmed && (
            <label className="check">
              <input type="checkbox" checked={propsConfirmed} onChange={(e) => setPropsConfirmed(e.target.checked)} />
              All propellers are off
            </label>
          )}
        </Panel>
      )}

      <ActionBar>
        {phase === "check" && capabilities && (
          <>
            <PadButton glyph="square" onPress={() => setPhase("test")}>
              Motor test
            </PadButton>
            {capabilities.endpoint_calibration_required ? (
              <PadButton glyph="cross" primary onPress={() => (setPropsConfirmed(false), setPhase("safety"))}>
                Calibrate ESCs
              </PadButton>
            ) : (
              <PadButton glyph="cross" primary onPress={() => (markComplete("esc"), nextStep("esc"))}>
                Done
              </PadButton>
            )}
          </>
        )}
        {phase === "safety" && (
          <>
            <PadButton glyph="circle" onPress={() => setPhase("check")}>
              Back
            </PadButton>
            <PadButton glyph="cross" primary disabled={!propsConfirmed} busy={busy} onPress={startGuided}>
              Begin
            </PadButton>
          </>
        )}
        {phase === "guided" && (
          <>
            <PadButton glyph="circle" onPress={cancelGuided}>
              Cancel
            </PadButton>
            <PadButton glyph="cross" primary busy={busy} onPress={advance}>
              {scene ? `Done: ${scene.title.toLowerCase()}` : "Next"}
            </PadButton>
          </>
        )}
        {testing && (
          <>
            <PadButton glyph="circle" onPress={() => setPhase("check")}>
              Back
            </PadButton>
            <PadButton glyph="cross" primary busy={busy || spinning} disabled={!propsConfirmed} onPress={spin}>
              Spin {MOTOR_LABELS[motor]}
            </PadButton>
          </>
        )}
      </ActionBar>
    </>
  );
}
