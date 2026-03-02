// Cyberpunk color theme matching the ABT GUI
export const theme = {
  bgDeep: "#0A0A12",
  bgPanel: "#10121E",
  bgCard: "#161A2A",
  bgCardHover: "#1C2236",
  accent: "#00FFEA",
  accentDim: "#00B4A5",
  secondary: "#FF00AA",
  success: "#00FF88",
  error: "#FF2A50",
  warning: "#FFD600",
  textPrimary: "#E0E4F0",
  textSecondary: "#828CAA",
  textMuted: "#46506E",
  border: "#283250",
  borderGlow: "#00FFEA",
  selection: "#00FFEA",
} as const;

// Shared CSS-in-JS helpers
export const fonts = {
  heading: "'Segoe UI', 'Inter', system-ui, sans-serif",
  mono: "'Cascadia Code', 'Fira Code', 'JetBrains Mono', monospace",
} as const;

export const glowText = (color: string, blur = 12) =>
  `0 0 ${blur}px ${color}, 0 0 ${blur * 2}px ${color}40`;

export const glowBox = (color: string, blur = 16) =>
  `0 0 ${blur}px ${color}60, inset 0 0 ${blur / 2}px ${color}20`;

export const scanlineOverlay: React.CSSProperties = {
  position: "absolute",
  inset: 0,
  background:
    "repeating-linear-gradient(0deg, transparent, transparent 2px, rgba(0,0,0,0.06) 2px, rgba(0,0,0,0.06) 4px)",
  pointerEvents: "none",
};
