import clsx from "clsx";
import { type ReactNode, useEffect, useRef } from "react";
import { sfx } from "../lib/sfx";
import { palette } from "../theme";

export type Glyph = "cross" | "circle" | "triangle" | "square";

const GLYPH_COLOR: Record<Glyph, string> = {
  cross: palette.cross,
  circle: palette.circle,
  triangle: palette.triangle,
  square: palette.square,
};

const GLYPH_KEYS: Record<Glyph, string[]> = {
  cross: ["Enter", " "],
  circle: ["Escape", "Backspace"],
  triangle: ["t", "T"],
  square: ["s", "S"],
};

export function GlyphIcon({ glyph, size = 22 }: { glyph: Glyph; size?: number }) {
  const color = GLYPH_COLOR[glyph];
  const stroke = { stroke: color, strokeWidth: 3.2, fill: "none", strokeLinecap: "round" as const, strokeLinejoin: "round" as const };
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden>
      {glyph === "cross" && <path d="M6 6 L18 18 M18 6 L6 18" {...stroke} />}
      {glyph === "circle" && <circle cx="12" cy="12" r="6.5" {...stroke} />}
      {glyph === "triangle" && <path d="M12 5 L19 17.5 L5 17.5 Z" {...stroke} />}
      {glyph === "square" && <rect x="6" y="6" width="12" height="12" rx="1" {...stroke} />}
    </svg>
  );
}

/**
 * A face-button style action. Each glyph doubles as a keyboard shortcut:
 * ✕ = Enter/Space, ○ = Esc, △ = T, □ = S.
 */
export function PadButton({
  glyph,
  children,
  onPress,
  disabled,
  busy,
  primary,
  hotkey = true,
}: {
  glyph: Glyph;
  children: ReactNode;
  onPress: () => void;
  disabled?: boolean;
  busy?: boolean;
  primary?: boolean;
  hotkey?: boolean;
}) {
  const press = useRef(onPress);
  press.current = onPress;
  const blocked = disabled || busy;

  useEffect(() => {
    if (!hotkey || blocked) return;
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.closest("input, select, textarea, [role=listbox]")) return;
      if (target.tagName === "BUTTON" && (e.key === "Enter" || e.key === " ")) return;
      if (GLYPH_KEYS[glyph].includes(e.key)) {
        e.preventDefault();
        sfx.tap();
        press.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [glyph, hotkey, blocked]);

  return (
    <button
      type="button"
      className={clsx("pad-button", primary && "pad-button--primary", busy && "pad-button--busy")}
      disabled={blocked}
      onClick={() => {
        sfx.tap();
        onPress();
      }}
    >
      <span className="pad-button__glyph">
        <GlyphIcon glyph={glyph} />
      </span>
      <span>{children}</span>
    </button>
  );
}

export function ActionBar({ children }: { children: ReactNode }) {
  return <div className="action-bar">{children}</div>;
}

/** Big chunky progress ring. `value` null = indeterminate shimmer. */
export function ProgressRing({
  value,
  color = palette.triangle,
  size = 132,
  label,
}: {
  value: number | null;
  color?: string;
  size?: number;
  label?: ReactNode;
}) {
  const r = 52;
  const circumference = 2 * Math.PI * r;
  const shown = value === null ? 0.25 : Math.max(0, Math.min(1, value));
  return (
    <div className="progress-ring" style={{ width: size, height: size }}>
      <svg viewBox="0 0 120 120" className={clsx(value === null && "progress-ring--spin")}>
        <circle cx="60" cy="60" r={r} stroke="rgba(255,255,255,0.18)" strokeWidth="12" fill="none" />
        <circle
          cx="60"
          cy="60"
          r={r}
          stroke={color}
          strokeWidth="12"
          strokeLinecap="round"
          fill="none"
          strokeDasharray={circumference}
          strokeDashoffset={circumference * (1 - shown)}
          transform="rotate(-90 60 60)"
          style={{ transition: "stroke-dashoffset 400ms cubic-bezier(.2,.9,.3,1.2)", filter: `drop-shadow(0 0 8px ${color})` }}
        />
      </svg>
      <div className="progress-ring__label">{label ?? (value === null ? "…" : `${Math.round(shown * 100)}%`)}</div>
    </div>
  );
}

/** Rounded card panel used on the left side of setup screens. */
export function Panel({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={clsx("panel", className)}>{children}</div>;
}

export function Title({ kicker, children }: { kicker?: string; children: ReactNode }) {
  return (
    <div className="title">
      {kicker && <div className="title__kicker">{kicker}</div>}
      <h1>{children}</h1>
    </div>
  );
}
