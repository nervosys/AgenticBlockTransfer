import React from "react";
import {
  AbsoluteFill,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { theme, fonts, glowText, scanlineOverlay } from "../theme";

// ─── Scene 6: Outro / CTA ────────────────────────────────────────────────
// Frames 0–120 (4 seconds)

export const OutroScene: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Diamond zoom
  const logoScale = spring({ frame, fps, config: { damping: 16, stiffness: 80 } });
  const logoOpacity = interpolate(frame, [0, 15], [0, 1], {
    extrapolateRight: "clamp",
  });

  // Title
  const titleOpacity = interpolate(frame, [15, 30], [0, 1], {
    extrapolateRight: "clamp",
  });

  // URL
  const urlOpacity = interpolate(frame, [35, 50], [0, 1], {
    extrapolateRight: "clamp",
  });
  const urlScale = spring({
    frame: frame - 35,
    fps,
    config: { damping: 14 },
  });

  // Stats row
  const statsOpacity = interpolate(frame, [55, 70], [0, 1], {
    extrapolateRight: "clamp",
  });

  // License
  const licenseOpacity = interpolate(frame, [70, 85], [0, 1], {
    extrapolateRight: "clamp",
  });

  // Fade out
  const fadeOut = interpolate(frame, [100, 120], [1, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  const stats = [
    { label: "Modules", value: "71" },
    { label: "Commands", value: "47" },
    { label: "Tests", value: "951" },
    { label: "Themes", value: "6" },
  ];

  return (
    <AbsoluteFill
      style={{
        backgroundColor: theme.bgDeep,
        justifyContent: "center",
        alignItems: "center",
        fontFamily: fonts.heading,
        opacity: fadeOut,
      }}
    >
      {/* Radial glow */}
      <div
        style={{
          position: "absolute",
          width: 800,
          height: 800,
          borderRadius: "50%",
          background: `radial-gradient(circle, ${theme.accent}10 0%, transparent 60%)`,
          opacity: logoOpacity,
        }}
      />

      {/* Diamond */}
      <div
        style={{
          opacity: logoOpacity,
          transform: `scale(${logoScale})`,
          fontSize: 64,
          color: theme.accent,
          textShadow: glowText(theme.accent, 20),
          marginBottom: 16,
        }}
      >
        ◆
      </div>

      {/* Title */}
      <div
        style={{
          opacity: titleOpacity,
          fontSize: 48,
          fontWeight: 800,
          color: theme.textPrimary,
          letterSpacing: 4,
          textShadow: glowText(theme.accent, 6),
        }}
      >
        ABT
      </div>

      {/* Stats row */}
      <div
        style={{
          opacity: statsOpacity,
          display: "flex",
          gap: 50,
          marginTop: 36,
        }}
      >
        {stats.map((s) => (
          <div key={s.label} style={{ textAlign: "center" }}>
            <div
              style={{
                fontSize: 30,
                fontWeight: 800,
                color: theme.accent,
                textShadow: glowText(theme.accent, 6),
              }}
            >
              {s.value}
            </div>
            <div
              style={{
                fontSize: 12,
                color: theme.textMuted,
                letterSpacing: 2,
                textTransform: "uppercase",
                marginTop: 4,
              }}
            >
              {s.label}
            </div>
          </div>
        ))}
      </div>

      {/* GitHub URL */}
      <div
        style={{
          opacity: urlOpacity,
          transform: `scale(${Math.max(0, urlScale)})`,
          marginTop: 40,
          padding: "12px 36px",
          borderRadius: 24,
          border: `1.5px solid ${theme.accent}80`,
          backgroundColor: `${theme.accent}08`,
          boxShadow: `0 0 20px ${theme.accent}20`,
        }}
      >
        <span
          style={{
            fontSize: 20,
            color: theme.accent,
            fontFamily: fonts.mono,
            letterSpacing: 1,
          }}
        >
          github.com/nervosys/AgenticBlockTransfer
        </span>
      </div>

      {/* License */}
      <div
        style={{
          opacity: licenseOpacity,
          marginTop: 24,
          fontSize: 14,
          color: theme.textMuted,
          letterSpacing: 2,
        }}
      >
        MIT / Apache-2.0 — Open Source
      </div>

      <div style={scanlineOverlay} />
    </AbsoluteFill>
  );
};
