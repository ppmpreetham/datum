import { useFrame } from "@react-three/fiber";
import clsx from "clsx";
import { useEffect, useMemo, useRef, useState } from "react";
import * as THREE from "three";
import { SceneTunnel, showError, speak, useBackend, useCamera, useDrone, useVehicle } from "../app/state";
import { ActionBar, PadButton, Panel, Title } from "../hud/Controls";
import { nedQuatToThree } from "../lib/attitude";
import { type RotationOption, rotationOptions, rotationParameter, rotationQuatNed } from "../lib/rotations";
import { sfx } from "../lib/sfx";
import { palette } from "../theme";
import { useArrowGeometry, FlightController, GpsPuck } from "../scene/models/FlightController";
import { Drone } from "../scene/models/Drone";
import { PulseRing } from "../scene/fx";

const GROUPS: { title: string; test: (o: RotationOption) => boolean }[] = [
  { title: "Flat", test: (o) => o.roll === 0 && o.pitch === 0 },
  { title: "Upside down", test: (o) => o.roll === 180 && o.pitch === 0 },
  { title: "On its side", test: (o) => (o.roll === 90 || o.roll === 270) && o.pitch === 0 },
  { title: "Tilted", test: () => true },
];

function groupOptions(options: RotationOption[]) {
  const used = new Set<number>();
  return GROUPS.map((group) => {
    const items = options.filter((o) => !used.has(o.value) && group.test(o));
    items.forEach((o) => used.add(o.value));
    return { title: group.title, items };
  }).filter((g) => g.items.length);
}

/** Dropdown that previews each option in 3D as you hover or arrow through it. */
export function RotationSelect({
  options,
  value,
  onChange,
  onPreview,
}: {
  options: RotationOption[];
  value: number;
  onChange: (value: number) => void;
  onPreview: (value: number | null) => void;
}) {
  const [open, setOpen] = useState(false);
  const [cursor, setCursor] = useState(value);
  const list = useRef<HTMLDivElement>(null);
  const groups = useMemo(() => groupOptions(options), [options]);
  const flat = useMemo(() => groups.flatMap((g) => g.items), [groups]);
  const current = options.find((o) => o.value === value);

  useEffect(() => {
    if (!open) return;
    setCursor(value);
    list.current?.focus();
    list.current?.querySelector(`[data-value="${value}"]`)?.scrollIntoView({ block: "center" });
  }, [open]);

  const choose = (v: number) => {
    sfx.pop(2);
    onChange(v);
    onPreview(null);
    setOpen(false);
  };

  return (
    <div className="select">
      <button type="button" className={clsx("select__button", open && "is-open")} onClick={() => (sfx.tap(), setOpen(!open))}>
        <span>{current?.label ?? "…"}</span>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round">
          <path d={open ? "M6 15l6-6 6 6" : "M6 9l6 6 6-6"} />
        </svg>
      </button>
      {open && (
        <div
          ref={list}
          className="select__list"
          role="listbox"
          tabIndex={-1}
          onMouseLeave={() => onPreview(null)}
          onKeyDown={(e) => {
            const index = flat.findIndex((o) => o.value === cursor);
            if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              const next = flat[Math.max(0, Math.min(flat.length - 1, index + (e.key === "ArrowDown" ? 1 : -1)))];
              setCursor(next.value);
              onPreview(next.value);
              list.current?.querySelector(`[data-value="${next.value}"]`)?.scrollIntoView({ block: "nearest" });
            } else if (e.key === "Enter") {
              e.preventDefault();
              choose(cursor);
            } else if (e.key === "Escape") {
              e.preventDefault();
              onPreview(null);
              setOpen(false);
            }
          }}
        >
          {groups.map((group) => (
            <div key={group.title}>
              <div className="select__group">{group.title}</div>
              {group.items.map((o) => (
                <div
                  key={o.value}
                  data-value={o.value}
                  role="option"
                  aria-selected={o.value === value}
                  className={clsx("select__option", o.value === value && "is-selected", o.value === cursor && "is-cursor")}
                  onMouseEnter={() => (setCursor(o.value), onPreview(o.value))}
                  onClick={() => choose(o.value)}
                >
                  {o.label}
                  {o.value === 0 && <span className="select__tag">forward</span>}
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** Ghost airframe with the board (or GPS puck) rotated by the chosen value. */
function MountPreview({ rotation, subject }: { rotation: RotationOption | undefined; subject: "board" | "gps" }) {
  const holder = useRef<THREE.Group>(null);
  const goal = useMemo(() => new THREE.Quaternion(), []);
  const arrow = useArrowGeometry(0.5, 0.32, 0.01);
  useFrame((_, dt) => {
    if (!holder.current || !rotation) return;
    nedQuatToThree(rotationQuatNed(rotation), goal);
    holder.current.quaternion.slerp(goal, 1 - Math.exp(-7 * dt));
  });
  const level = useRef(new THREE.Quaternion());
  return (
    <group>
      <Drone ghost={palette.glow} opacity={0.9} target={level} />
      {/* Vehicle forward, painted on the floor. */}
      <mesh geometry={arrow} position={[0, -0.53, -0.95]}>
        <meshBasicMaterial color={palette.glow} toneMapped={false} transparent opacity={0.85} />
      </mesh>
      <group position={subject === "board" ? [0, 0.32, 0] : [0, 0.62, 0.25]}>
        <group ref={holder} scale={subject === "board" ? 1.35 : 1.4}>
          {subject === "board" ? <FlightController /> : <GpsPuck mast={false} />}
        </group>
        <PulseRing radius={0.55} color={palette.gold} speed={0.5} position={[0, -0.12, 0]} />
      </group>
      {subject === "gps" && (
        <mesh position={[0, 0.25, 0.25]}>
          <cylinderGeometry args={[0.015, 0.015, 0.7, 12]} />
          <meshStandardMaterial color={palette.graphite} />
        </mesh>
      )}
    </group>
  );
}

/** Camera framing that leaves the left half of the screen for the panel. */
const SIDE_SHOT = { position: [-1.05, 1.5, 3.7] as [number, number, number], target: [-1.0, 0.1, 0] as [number, number, number] };

/**
 * QGroundControl asks for "Autopilot Rotation" before calibrating; so do we.
 * The value is read from the vehicle, previewed in 3D, and written back only
 * if it changed.
 */
export function OrientationSetup({
  sensorName,
  onConfirm,
}: {
  sensorName: string;
  onConfirm: () => void;
}) {
  const backend = useBackend();
  const vehicle = useVehicle();
  const options = useMemo(() => rotationOptions(vehicle.firmware), [vehicle.firmware]);
  const parameter = rotationParameter(vehicle.firmware, "board");
  const [original, setOriginal] = useState<number | null>(null);
  const [value, setValue] = useState(0);
  const [preview, setPreview] = useState<number | null>(null);
  const [busy, setBusy] = useState(true);
  const [readFailed, setReadFailed] = useState(false);

  useDrone({ visible: false });
  useCamera(SIDE_SHOT.position, SIDE_SHOT.target);

  useEffect(() => {
    speak("Which way is your flight controller facing?");
    let cancelled = false;
    backend
      .readParameters([parameter])
      .then((values) => {
        if (cancelled) return;
        const current = values[parameter] ?? 0;
        setOriginal(current);
        setValue(current);
      })
      .catch((error) => {
        if (cancelled) return;
        setReadFailed(true);
        showError(error);
      })
      .finally(() => !cancelled && setBusy(false));
    return () => {
      cancelled = true;
    };
  }, [backend, parameter]);

  const confirm = async () => {
    if (original !== null && value === original) {
      sfx.ding();
      onConfirm();
      return;
    }
    setBusy(true);
    try {
      await backend.writeParameters({ [parameter]: value });
      setOriginal(value);
      sfx.ding();
      speak("Saved!", "good");
      onConfirm();
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  const shown = options.find((o) => o.value === (preview ?? value));

  return (
    <>
      <SceneTunnel.In>
        <MountPreview rotation={shown} subject="board" />
      </SceneTunnel.In>
      <Panel className="panel--left">
        <Title kicker={`${sensorName} · before we start`}>Autopilot rotation</Title>
        <p className="hint">Gold arrow pointing the same way as the drone's nose? Keep it on None.</p>
        <RotationSelect options={options} value={value} onChange={setValue} onPreview={setPreview} />
        <div className="param-tag">
          {parameter}
          {original !== null && original !== value && <span className="param-tag__changed">will change</span>}
        </div>
      </Panel>
      <ActionBar>
        <PadButton glyph="cross" primary busy={busy} onPress={confirm}>
          {readFailed ? "Save and continue" : "Yes, that's it"}
        </PadButton>
      </ActionBar>
    </>
  );
}

/**
 * QGroundControl's compass block: internal vs external, plus orientation for
 * external compasses (hidden when the autopilot auto-detects it).
 */
export function CompassMountSetup({ onConfirm, onBack }: { onConfirm: () => void; onBack: () => void }) {
  const backend = useBackend();
  const vehicle = useVehicle();
  const px4 = vehicle.firmware === "px4";
  const options = useMemo(() => rotationOptions(vehicle.firmware), [vehicle.firmware]);
  const orientParameter = rotationParameter(vehicle.firmware, "compass");
  const [external, setExternal] = useState(true);
  const [orientation, setOrientation] = useState(0);
  const [autoRotation, setAutoRotation] = useState(false);
  const [original, setOriginal] = useState<Record<string, number>>({});
  const [preview, setPreview] = useState<number | null>(null);
  const [busy, setBusy] = useState(true);

  useDrone({ visible: false });
  useCamera(SIDE_SHOT.position, SIDE_SHOT.target);

  useEffect(() => {
    speak("Where does your compass live?");
    let cancelled = false;
    const names = px4 ? ["CAL_MAG0_ROT"] : ["COMPASS_EXTERNAL", "COMPASS_ORIENT"];
    (async () => {
      try {
        const values = await backend.readParameters(names);
        let auto = false;
        if (!px4) {
          // Optional on older firmware; missing just means "not auto".
          auto = await backend
            .readParameters(["COMPASS_AUTO_ROT"])
            .then((v) => v.COMPASS_AUTO_ROT === 2)
            .catch(() => false);
        }
        if (cancelled) return;
        setOriginal(values);
        setAutoRotation(auto);
        if (px4) {
          setExternal((values.CAL_MAG0_ROT ?? -1) >= 0);
          setOrientation(Math.max(0, values.CAL_MAG0_ROT ?? 0));
        } else {
          setExternal((values.COMPASS_EXTERNAL ?? 1) > 0);
          setOrientation(values.COMPASS_ORIENT ?? 0);
        }
      } catch (error) {
        if (!cancelled) showError(error);
      } finally {
        if (!cancelled) setBusy(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [backend, px4]);

  const confirm = async () => {
    const changes: Record<string, number> = {};
    if (px4) {
      // PX4 only stores a rotation for external magnetometers.
      if (external && original.CAL_MAG0_ROT !== orientation) changes.CAL_MAG0_ROT = orientation;
    } else {
      const ext = external ? 1 : 0;
      if (original.COMPASS_EXTERNAL !== ext && original.COMPASS_EXTERNAL !== 2) changes.COMPASS_EXTERNAL = ext;
      if (external && !autoRotation && original.COMPASS_ORIENT !== orientation) changes.COMPASS_ORIENT = orientation;
    }
    setBusy(true);
    try {
      if (Object.keys(changes).length) {
        await backend.writeParameters(changes);
        speak("Saved!", "good");
      }
      sfx.ding();
      onConfirm();
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  const shown = options.find((o) => o.value === (preview ?? orientation));
  const none = options[0];

  return (
    <>
      <SceneTunnel.In>
        <MountPreview rotation={external ? shown : none} subject={external ? "gps" : "board"} />
      </SceneTunnel.In>
      <Panel className="panel--left">
        <Title kicker="Compass · before we start">Inside or outside?</Title>
        <div className="choice-row">
          <button type="button" className={clsx("choice", !external && "is-selected")} onClick={() => (sfx.pop(1), setExternal(false))}>
            <span className="choice__icon">▣</span>
            Internal
            <small>in the flight controller</small>
          </button>
          <button type="button" className={clsx("choice", external && "is-selected")} onClick={() => (sfx.pop(3), setExternal(true))}>
            <span className="choice__icon">◉</span>
            External
            <small>GPS puck on a mast</small>
          </button>
        </div>
        {external &&
          (autoRotation ? (
            <p className="hint hint--good">Orientation is auto-detected by the autopilot.</p>
          ) : (
            <>
              <p className="hint">Which way does the puck's arrow point?</p>
              <RotationSelect options={options} value={orientation} onChange={setOrientation} onPreview={setPreview} />
              <div className="param-tag">{orientParameter}</div>
            </>
          ))}
        {!px4 && original.COMPASS_EXTERNAL === 2 && <p className="hint">Forced external by COMPASS_EXTERNAL = 2.</p>}
      </Panel>
      <ActionBar>
        <PadButton glyph="circle" onPress={onBack}>
          Back
        </PadButton>
        <PadButton glyph="cross" primary busy={busy} onPress={confirm}>
          Yes, continue
        </PadButton>
      </ActionBar>
    </>
  );
}
