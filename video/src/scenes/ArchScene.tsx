import React from "react";
import {
  AbsoluteFill,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { theme, fonts, glowText, glowBox, scanlineOverlay } from "../theme";

// ─── Scene 5: Architecture overview ──────────────────────────────────────
// Frames 0–180 (6 seconds)

const layers = [
  {
    label: "GUI",
    sub: "egui / eframe",
    icon: "◆",
    color: theme.accent,
  },
  {
    label: "TUI",
    sub: "ratatui / crossterm",
    icon: "▤",
    color: theme.secondary,
  },
  {
    label: "CLI",
    sub: "clap v4 — 47 commands",
    icon: ">_",
    color: theme.warning,
  },
  {
    label: "Core Engine",
    sub: "71 modules — async Rust + tokio",
    icon: "⚙",
    color: theme.success,
  },
  {
    label: "Platform Layer",
    sub: "Windows · macOS · Linux",
    icon: "♨",
    color: theme.error,
  },
];

export const ArchScene: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const titleOpacity = interpolate(frame, [0, 15], [0, 1], {
    extrapolateRight: "clamp",
  });

  return (
    <AbsoluteFill
      style={{
        backgroundColor: theme.bgDeep,
        fontFamily: fonts.heading,
        padding: 80,
      }}
    >
      {/* Section title */}
      <div
        style={{
          opacity: titleOpacity,
          fontSize: 42,
          fontWeight: 700,
          color: theme.textPrimary,
          marginBottom: 50,
          display: "flex",
          alignItems: "center",
          gap: 16,
        }}
      >
        <span style={{ color: theme.accent, textShadow: glowText(theme.accent, 10) }}>
          ◇
        </span>
        Architecture
      </div>

      {/* Stacked layers */}
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 8,
          maxWidth: 900,
          margin: "0 auto",
        }}
      >
        {layers.map((layer, i) => {
          const delay = 20 + i * 20;
          const layerScale = spring({
            frame: frame - delay,
            fps,
            config: { damping: 14, stiffness: 90 },
          });
          const layerOpacity = interpolate(
            frame,
            [delay, delay + 12],
            [0, 1],
            { extrapolateRight: "clamp" }
          );

          // Widths get wider as we go down the stack
          const widthPct = 60 + i * 10;

          return (
            <React.Fragment key={layer.label}>
              <div
                style={{
                  opacity: layerOpacity,
                  transform: `scale(${Math.max(0, layerScale)})`,
                  width: `${widthPct}%`,
                  backgroundColor: `${layer.color}12`,
                  border: `1.5px solid ${layer.color}50`,
                  borderRadius: 12,
                  padding: "20px 32px",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  boxShadow: layerOpacity > 0.8 ? glowBox(layer.color, 8) : "none",
                }}
              >
                <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
                  <span
                    style={{
                      fontSize: 28,
                      color: layer.color,
                      textShadow: glowText(layer.color, 8),
                    }}
                  >
                    {layer.icon}
                  </span>
                  <div>
                    <div
                      style={{
                        fontSize: 22,
                        fontWeight: 700,
                        color: theme.textPrimary,
                      }}
                    >
                      {layer.label}
                    </div>
                    <div
                      style={{
                        fontSize: 14,
                        color: theme.textSecondary,
                        fontFamily: fonts.mono,
                        marginTop: 2,
                      }}
                    >
                      {layer.sub}
                    </div>
                  </div>
                </div>
              </div>

              {/* Connector line */}
              {i < layers.length - 1 && (
                <div
                  style={{
                    width: 2,
                    height: 16,
                    backgroundColor: layerOpacity > 0.5 ? `${layer.color}50` : "transparent",
                  }}
                />
              )}
            </React.Fragment>
          );
        })}
      </div>

      <div style={scanlineOverlay} />
    </AbsoluteFill>
  );
};
