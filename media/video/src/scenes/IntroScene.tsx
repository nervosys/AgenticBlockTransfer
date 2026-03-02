import React from "react";
import {
  AbsoluteFill,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { theme, fonts, glowText, scanlineOverlay } from "../theme";

// ─── Scene 1: Intro / Hero ────────────────────────────────────────────────
// Frames 0–150 (5 seconds at 30fps)
export const IntroScene: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Diamond logo animation
  const logoScale = spring({ frame, fps, config: { damping: 14, stiffness: 80 } });
  const logoRotate = interpolate(frame, [0, 60], [180, 0], { extrapolateRight: "clamp" });
  const logoOpacity = interpolate(frame, [0, 20], [0, 1], { extrapolateRight: "clamp" });

  // Title stagger
  const titleY = spring({ frame: frame - 15, fps, config: { damping: 16 } });
  const titleOpacity = interpolate(frame, [15, 35], [0, 1], { extrapolateRight: "clamp" });

  // Subtitle fade
  const subOpacity = interpolate(frame, [40, 60], [0, 1], { extrapolateRight: "clamp" });
  const subY = interpolate(frame, [40, 60], [20, 0], { extrapolateRight: "clamp" });

  // Version badge
  const badgeScale = spring({ frame: frame - 60, fps, config: { damping: 12 } });

  // Tagline
  const tagOpacity = interpolate(frame, [80, 100], [0, 1], { extrapolateRight: "clamp" });

  // Grid lines animation
  const gridOpacity = interpolate(frame, [0, 30], [0, 0.15], { extrapolateRight: "clamp" });

  return (
    <AbsoluteFill
      style={{
        backgroundColor: theme.bgDeep,
        justifyContent: "center",
        alignItems: "center",
        fontFamily: fonts.heading,
      }}
    >
      {/* Animated grid background */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          opacity: gridOpacity,
          backgroundImage: `
            linear-gradient(${theme.accent}20 1px, transparent 1px),
            linear-gradient(90deg, ${theme.accent}20 1px, transparent 1px)
          `,
          backgroundSize: "60px 60px",
          backgroundPosition: `0 ${frame * 0.3}px`,
        }}
      />

      {/* Radial glow behind logo */}
      <div
        style={{
          position: "absolute",
          width: 600,
          height: 600,
          borderRadius: "50%",
          background: `radial-gradient(circle, ${theme.accent}15 0%, transparent 70%)`,
          opacity: logoOpacity,
          transform: `scale(${logoScale})`,
        }}
      />

      {/* Diamond logo */}
      <div
        style={{
          opacity: logoOpacity,
          transform: `scale(${logoScale}) rotate(${logoRotate}deg)`,
          fontSize: 80,
          color: theme.accent,
          textShadow: glowText(theme.accent, 20),
          marginBottom: 20,
        }}
      >
        ◆
      </div>

      {/* Title */}
      <div
        style={{
          opacity: titleOpacity,
          transform: `translateY(${interpolate(titleY, [0, 1], [30, 0])}px)`,
          fontSize: 72,
          fontWeight: 800,
          color: theme.textPrimary,
          letterSpacing: 6,
          textShadow: glowText(theme.accent, 8),
        }}
      >
        ABT
      </div>

      {/* Subtitle */}
      <div
        style={{
          opacity: subOpacity,
          transform: `translateY(${subY}px)`,
          fontSize: 24,
          color: theme.textSecondary,
          letterSpacing: 4,
          marginTop: 8,
        }}
      >
        AGENTICBLOCKTRANSFER
      </div>

      {/* Version badge */}
      <div
        style={{
          marginTop: 30,
          transform: `scale(${Math.max(0, badgeScale)})`,
          padding: "8px 28px",
          borderRadius: 20,
          border: `1.5px solid ${theme.accent}`,
          color: theme.accent,
          fontSize: 16,
          fontWeight: 600,
          letterSpacing: 2,
          boxShadow: `0 0 16px ${theme.accent}40`,
        }}
      >
        v1.6.0
      </div>

      {/* Tagline */}
      <div
        style={{
          opacity: tagOpacity,
          marginTop: 28,
          fontSize: 18,
          color: theme.textMuted,
          fontStyle: "italic",
          maxWidth: 700,
          textAlign: "center",
          lineHeight: 1.5,
        }}
      >
        The agentic-first disk writer for the modern era
      </div>

      {/* Scanline overlay */}
      <div style={scanlineOverlay} />
    </AbsoluteFill>
  );
};
