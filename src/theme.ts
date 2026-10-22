/**
 * Astro's Playroom inspired palette: a bright PlayStation-blue sky, white
 * glossy hardware, and the four face-button colours as accents.
 */
export const palette = {
  skyTop: "#0099cc",
  skyBottom: "#024da1",
  deep: "#01295f",
  ink: "#0a1f44",
  white: "#ffffff",
  glow: "#9ff4ff",
  cyan: "#3fd0ff",
  triangle: "#3ee6b0",
  circle: "#ff5c7a",
  cross: "#7fb2ff",
  square: "#ff8ad8",
  gold: "#ffd447",
  plastic: "#f4f7ff",
  graphite: "#2a3550",
} as const;

export const symbolColors = [palette.triangle, palette.circle, palette.cross, palette.square];
