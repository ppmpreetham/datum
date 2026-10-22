import clsx from "clsx";
import { useEffect, useMemo, useRef, useState } from "react";
import { SceneTunnel, nextStep, showError, speak, useBackend, useCamera, useDrone, useVehicle } from "../app/state";
import { ActionBar, PadButton, Panel, Title } from "../hud/Controls";
import { setSimChannel, simRadioInputs } from "../lib/sim";
import { sfx } from "../lib/sfx";
import type { RadioChannel, RadioStatus } from "../lib/types";
import { PulseRing } from "../scene/fx";
import { Receiver, type StickGhost, Transmitter, makeTxState } from "../scene/models/Radio";
import { palette, symbolColors } from "../theme";
import { DoneCard, FailCard } from "./common";

type Fn = "throttle" | "yaw" | "roll" | "pitch";
type Mapping = Partial<Record<Fn, { channel: number; reversed: boolean }>>;

/** QGroundControl's stick sequence (mode 2). */
const MOVES: { id: string; fn: Fn; dir: 1 | -1; detect: boolean; say: string; hand: "left" | "right"; ghost: StickGhost }[] = [
  { id: "throttle_up", fn: "throttle", dir: 1, detect: true, say: "Throttle all the way up!", hand: "left", ghost: { x: 0, y: 1 } },
  { id: "throttle_down", fn: "throttle", dir: -1, detect: false, say: "And all the way back down.", hand: "left", ghost: { x: 0, y: -1 } },
  { id: "yaw_right", fn: "yaw", dir: 1, detect: true, say: "Left stick — push it right.", hand: "left", ghost: { x: 1, y: -1 } },
  { id: "yaw_left", fn: "yaw", dir: -1, detect: false, say: "Now push it left.", hand: "left", ghost: { x: -1, y: -1 } },
  { id: "roll_right", fn: "roll", dir: 1, detect: true, say: "Right stick — push it right.", hand: "right", ghost: { x: 1, y: 0 } },
  { id: "roll_left", fn: "roll", dir: -1, detect: false, say: "Now left.", hand: "right", ghost: { x: -1, y: 0 } },
  { id: "pitch_up", fn: "pitch", dir: 1, detect: true, say: "Right stick up!", hand: "right", ghost: { x: 0, y: 1 } },
  { id: "pitch_down", fn: "pitch", dir: -1, detect: false, say: "And down.", hand: "right", ghost: { x: 0, y: -1 } },
];

const DEFAULT_MAPPING: Required<Mapping> = {
  roll: { channel: 1, reversed: false },
  pitch: { channel: 2, reversed: false },
  throttle: { channel: 3, reversed: false },
  yaw: { channel: 4, reversed: false },
};

const DETECT_US = 350;

function find(channels: RadioChannel[], channel: number) {
  return channels.find((c) => c.channel === channel);
}

/** -1..1 for a stick function, using the captured trim as centre. */
function normalized(channels: RadioChannel[], fn: Fn, mapping: Mapping) {
  const map = mapping[fn] ?? DEFAULT_MAPPING[fn];
  const c = find(channels, map.channel);
  if (!c) return fn === "throttle" ? -1 : 0;
  const sign = map.reversed ? -1 : 1;
  if (fn === "throttle") return Math.max(-1, Math.min(1, ((c.value_us - 1500) / 500) * sign));
  return Math.max(-1, Math.min(1, ((c.value_us - c.trim_us) / 500) * sign));
}

type Phase = "power" | "center" | "moves" | "switches" | "saving" | "done" | "failed" | "unsupported";

const TX_POSITION: [number, number, number] = [6, 0.15, 0];

export function RadioStep() {
  const backend = useBackend();
  const vehicle = useVehicle();
  const [phase, setPhase] = useState<Phase>(vehicle.firmware === "px4" ? "unsupported" : "power");
  const [radio, setRadio] = useState<RadioStatus | null>(null);
  const [moveIndex, setMoveIndex] = useState(0);
  const [mapping, setMapping] = useState<Mapping>({});
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const tx = useRef(makeTxState());
  const holdTicks = useRef(0);
  const sim = backend.kind === "sim";

  useDrone({ pose: "level", visible: true });
  useCamera([TX_POSITION[0] + 0.45, 0.6, 4.0], [TX_POSITION[0] + 0.45, 0.02, 0]);

  useEffect(() => {
    void backend.radioChannels().then(setRadio).catch(() => undefined);
    return backend.onRadio(setRadio);
  }, [backend]);

  const channels = radio?.channels ?? [];
  const linked = !!radio?.signal_available && channels.length > 0;

  // Keep the 3D transmitter in sync with telemetry.
  useEffect(() => {
    const t = tx.current;
    t.throttle = normalized(channels, "throttle", mapping);
    t.yaw = normalized(channels, "yaw", mapping);
    t.roll = normalized(channels, "roll", mapping);
    t.pitch = normalized(channels, "pitch", mapping);
    t.channels = [1, 2, 3, 4, 5, 6].map((n) => find(channels, n)?.value_us ?? 1500);
    if (sim) {
      t.switches[0] = simRadioInputs[4] > 1500 ? 1 : -1;
      t.switches[2] = simRadioInputs[5] > 1700 ? 1 : simRadioInputs[5] > 1300 ? 0 : -1;
    }
  }, [radio, mapping, sim]);

  useEffect(() => {
    if (phase === "power") speak("Switch on your FS-i6. Is the receiver light solid?");
    if (phase === "center") speak("Center the sticks, throttle all the way down.");
    if (phase === "switches") speak("Flip every switch and twist both knobs, end to end.");
    if (phase === "unsupported") speak("Radio setup here needs ArduPilot.", "warn");
  }, [phase]);

  const move = MOVES[moveIndex];
  useEffect(() => {
    if (phase === "moves" && move) speak(move.say);
  }, [phase, moveIndex]);

  // Stick-move detection, QGroundControl style.
  useEffect(() => {
    if (phase !== "moves" || !move) return;
    let reached = false;
    let learned: Mapping[Fn] | undefined;
    if (move.detect) {
      const taken = new Set(Object.values(mapping).map((m) => m!.channel));
      let best: RadioChannel | undefined;
      for (const c of channels) {
        if (taken.has(c.channel)) continue;
        if (!best || Math.abs(c.value_us - c.trim_us) > Math.abs(best.value_us - best.trim_us)) best = c;
      }
      if (best && Math.abs(best.value_us - best.trim_us) > DETECT_US) {
        reached = true;
        learned = { channel: best.channel, reversed: best.value_us < best.trim_us };
      }
    } else {
      const map = mapping[move.fn];
      const c = map && find(channels, map.channel);
      if (c) {
        if (move.fn === "throttle") reached = Math.abs(c.value_us - c.trim_us) < 120;
        else reached = normalized(channels, move.fn, mapping) * move.dir > 0.7;
      }
    }
    holdTicks.current = reached ? holdTicks.current + 1 : 0;
    if (holdTicks.current >= 3) {
      holdTicks.current = 0;
      if (learned) setMapping((m) => ({ ...m, [move.fn]: learned }));
      sfx.pop(moveIndex);
      if (moveIndex + 1 >= MOVES.length) {
        sfx.ding();
        setPhase("switches");
      } else setMoveIndex(moveIndex + 1);
    }
  }, [radio, phase, moveIndex]);

  const centered = (() => {
    const get = (n: number) => find(channels, n)?.value_us ?? 1500;
    return [1, 2, 4].every((n) => Math.abs(get(n) - 1500) < 120) && get(3) < 1150;
  })();

  const start = async () => {
    setBusy(true);
    try {
      const status = await backend.startRadio();
      setRadio(status);
      setMapping({});
      setMoveIndex(0);
      setPhase("moves");
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (!radio?.session_id) return;
    setBusy(true);
    setPhase("saving");
    speak("Saving…");
    try {
      setRadio(await backend.finishRadio(radio.session_id));
      setPhase("done");
    } catch (error) {
      const text = error instanceof Error ? error.message : String(error);
      if (text.startsWith("INVALID_RC_RANGE")) {
        showError(error);
        setPhase("switches");
      } else {
        setFailure(text);
        setPhase("failed");
      }
    } finally {
      setBusy(false);
    }
  };

  const cancel = async () => {
    if (radio?.session_id) await backend.cancelRadio(radio.session_id).catch(showError);
    setPhase("center");
  };

  // Simulator: drag the 3D sticks, click the switches.
  const dragValues = useRef({ left: { x: 1500, y: 1000 }, right: { x: 1500, y: 1500 } });
  const onStickDrag = (hand: "left" | "right", dx: number, dy: number, done: boolean) => {
    const v = dragValues.current[hand];
    if (done) {
      v.x = 1500;
      if (hand === "right") v.y = 1500;
    } else {
      v.x = Math.max(1000, Math.min(2000, v.x + dx * 500));
      v.y = Math.max(1000, Math.min(2000, v.y + dy * 500));
    }
    if (hand === "left") {
      setSimChannel(4, v.x);
      setSimChannel(3, v.y);
    } else {
      setSimChannel(1, v.x);
      setSimChannel(2, v.y);
    }
  };
  const onSwitch = (index: number) => {
    sfx.tap();
    if (index === 0 || index === 1) setSimChannel(5, simRadioInputs[4] > 1500 ? 1000 : 2000);
    else setSimChannel(6, simRadioInputs[5] >= 2000 ? 1000 : simRadioInputs[5] + 500);
  };

  const ghost = phase === "moves" && move ? move : null;
  const centerGhosts = phase === "center";
  const aux = useMemo(() => {
    const used = new Set(Object.values({ ...DEFAULT_MAPPING, ...mapping }).map((m) => m.channel));
    return channels.filter((c) => !used.has(c.channel));
  }, [channels, mapping]);

  if (phase === "done") {
    return (
      <DoneCard step="radio" title="Radio is dialed in!" onRedo={() => setPhase("center")}>
        <ChannelBars channels={channels} compact />
      </DoneCard>
    );
  }
  if (phase === "failed") return <FailCard message={failure} onRetry={() => setPhase("center")} />;

  return (
    <>
      <SceneTunnel.In>
        <group position={TX_POSITION}>
          <Transmitter
            state={tx}
            ghostLeft={ghost?.hand === "left" ? ghost.ghost : centerGhosts ? { x: 0, y: -1 } : null}
            ghostRight={ghost?.hand === "right" ? ghost.ghost : centerGhosts ? { x: 0, y: 0 } : null}
            highlight={phase === "switches" ? "switches" : ghost ? ghost.hand : centerGhosts ? "both" : null}
            onStickDrag={sim ? onStickDrag : undefined}
            onSwitch={sim ? onSwitch : undefined}
          />
        </group>
        <group position={[TX_POSITION[0] + 1.45, -0.45, 0.35]} rotation={[0, -0.5, 0]}>
          <Receiver linked={linked} />
          {phase === "power" && <PulseRing radius={0.4} color={linked ? palette.triangle : palette.circle} speed={0.8} position={[0, -0.08, 0]} />}
        </group>
      </SceneTunnel.In>

      {phase === "unsupported" ? (
        <Panel className="panel--left">
          <Title kicker="Radio">Not for PX4 yet</Title>
          <p className="hint">Datum's radio capture writes ArduPilot RCx parameters. Use QGroundControl's Radio page for PX4.</p>
        </Panel>
      ) : phase === "power" ? (
        <Panel className="panel--left">
          <Title kicker="Radio · FS-i6 + FS-iA6B">Wake up the radio</Title>
          <p className="hint">{linked ? "Signal! The receiver is talking to the flight controller." : "Waiting for RC signal… bind the receiver if the light keeps blinking."}</p>
          {sim && <p className="hint hint--small">Simulator: drag the sticks, click the switches.</p>}
        </Panel>
      ) : (
        <div className="side-hud side-hud--left">
          {phase === "moves" && (
            <div className="move-dots">
              {MOVES.map((m, i) => (
                <span key={m.id} className={clsx("orb-dot", i < moveIndex && "is-hit", i === moveIndex && "is-now")} style={{ "--dot": symbolColors[i % 4] } as React.CSSProperties} />
              ))}
            </div>
          )}
          {(phase === "switches" || phase === "saving") && <ChannelBars channels={aux} />}
        </div>
      )}

      <ActionBar>
        {phase === "unsupported" && (
          <PadButton glyph="cross" primary onPress={() => nextStep("radio")}>
            Skip
          </PadButton>
        )}
        {phase === "power" && (
          <PadButton glyph="cross" primary onPress={() => (sfx.ding(), setPhase("center"))}>
            {linked ? "It's on" : "Continue anyway"}
          </PadButton>
        )}
        {phase === "center" && (
          <PadButton glyph="cross" primary busy={busy} disabled={!linked} onPress={start}>
            {centered ? "Sticks centered" : "Ready"}
          </PadButton>
        )}
        {(phase === "moves" || phase === "switches") && (
          <PadButton glyph="circle" onPress={cancel}>
            Cancel
          </PadButton>
        )}
        {phase === "moves" && (
          <PadButton glyph="square" onPress={() => setPhase("switches")}>
            Skip sticks
          </PadButton>
        )}
        {(phase === "switches" || phase === "saving") && (
          <PadButton glyph="cross" primary busy={busy} onPress={save}>
            Save
          </PadButton>
        )}
      </ActionBar>
    </>
  );
}

/** Captured range per channel: the bar is min→max, the tick is live. */
function ChannelBars({ channels, compact }: { channels: RadioChannel[]; compact?: boolean }) {
  return (
    <div className={clsx("channel-bars", compact && "channel-bars--compact")}>
      {channels.map((c, i) => {
        const pos = (v: number) => `${Math.max(0, Math.min(1, (v - 900) / 1200)) * 100}%`;
        const travel = c.maximum_us - c.minimum_us;
        const state = travel >= 400 ? "full" : travel < 100 ? "idle" : "partial";
        return (
          <div key={c.channel} className={clsx("channel-bar", `channel-bar--${state}`)} style={{ "--bar": symbolColors[i % 4] } as React.CSSProperties}>
            <span>CH{c.channel}</span>
            <div className="channel-bar__track">
              <i style={{ left: pos(c.minimum_us), width: `calc(${pos(c.maximum_us)} - ${pos(c.minimum_us)})` }} />
              <b style={{ left: pos(c.value_us) }} />
            </div>
            <small>{state === "full" ? "✓" : state === "idle" ? "unused" : `${travel}µs`}</small>
          </div>
        );
      })}
    </div>
  );
}
