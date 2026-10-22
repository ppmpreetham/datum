/**
 * Tiny synthesized sound kit (no audio assets) plus spoken prompts. Short,
 * bright, toy-like sounds in the spirit of Astro's Playroom.
 */
let ctx: AudioContext | null = null;
let muted = false;
let voiceEnabled = true;

function audio() {
  if (!ctx) ctx = new AudioContext();
  if (ctx.state === "suspended") void ctx.resume();
  return ctx;
}

function tone(
  frequency: number,
  duration: number,
  { type = "sine", gain = 0.12, delay = 0, slideTo }: {
    type?: OscillatorType;
    gain?: number;
    delay?: number;
    slideTo?: number;
  } = {},
) {
  if (muted) return;
  const a = audio();
  const t = a.currentTime + delay;
  const osc = a.createOscillator();
  const amp = a.createGain();
  osc.type = type;
  osc.frequency.setValueAtTime(frequency, t);
  if (slideTo) osc.frequency.exponentialRampToValueAtTime(slideTo, t + duration);
  amp.gain.setValueAtTime(0, t);
  amp.gain.linearRampToValueAtTime(gain, t + 0.01);
  amp.gain.exponentialRampToValueAtTime(0.0001, t + duration);
  osc.connect(amp).connect(a.destination);
  osc.start(t);
  osc.stop(t + duration + 0.05);
}

export const sfx = {
  tap: () => tone(880, 0.08, { type: "triangle", gain: 0.08 }),
  pop: (pitch = 0) => tone(520 + pitch * 40, 0.12, { slideTo: 1100 + pitch * 60, gain: 0.07 }),
  whoosh: () => tone(300, 0.35, { type: "sine", slideTo: 900, gain: 0.04 }),
  ding: () => {
    tone(1046, 0.25, { type: "triangle" });
    tone(1568, 0.4, { type: "triangle", delay: 0.09 });
  },
  success: () => {
    [523, 659, 784, 1046].forEach((f, i) => tone(f, 0.3, { type: "triangle", delay: i * 0.09 }));
  },
  error: () => {
    tone(330, 0.18, { type: "square", gain: 0.05 });
    tone(247, 0.3, { type: "square", gain: 0.05, delay: 0.15 });
  },
  /** ESC-style beeps: `pattern` is a list of [frequency, ms] pairs. */
  escBeeps: (pattern: [number, number][]) => {
    let delay = 0;
    for (const [frequency, ms] of pattern) {
      tone(frequency, ms / 1000, { type: "square", gain: 0.04, delay });
      delay += ms / 1000 + 0.08;
    }
  },
};

export function setMuted(value: boolean) {
  muted = value;
  if (value) window.speechSynthesis?.cancel();
}

export function setVoice(value: boolean) {
  voiceEnabled = value;
  if (!value) window.speechSynthesis?.cancel();
}

let lastSpoken = "";
let preferredVoice: SpeechSynthesisVoice | null = null;

function pickVoice() {
  const voices = window.speechSynthesis?.getVoices() ?? [];
  preferredVoice =
    voices.find((v) => /en/i.test(v.lang) && /(aria|jenny|natural|samantha|google us)/i.test(v.name)) ??
    voices.find((v) => /^en/i.test(v.lang)) ??
    null;
}
if (typeof window !== "undefined" && window.speechSynthesis) {
  pickVoice();
  window.speechSynthesis.addEventListener?.("voiceschanged", pickVoice);
}

/** Speak a short line. Repeating the same line back-to-back is ignored. */
export function say(text: string) {
  if (muted || !voiceEnabled || !window.speechSynthesis || text === lastSpoken) return;
  lastSpoken = text;
  window.speechSynthesis.cancel();
  const utterance = new SpeechSynthesisUtterance(text);
  if (preferredVoice) utterance.voice = preferredVoice;
  utterance.rate = 1.04;
  utterance.pitch = 1.15;
  window.speechSynthesis.speak(utterance);
}
