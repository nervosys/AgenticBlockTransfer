import React from "react";
import {
  AbsoluteFill,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { theme, fonts, glowText, glowBox, scanlineOverlay } from "../theme";

// ─── Scene 2: Features showcase ───────────────────────────────────────────
// Frames 0–240 (8 seconds)

const features = [
  {
    icon: "⚡",
    title: "Cross-Platform",
    desc: "Windows, macOS, Linux — one binary",
    color: theme.accent,
  },
  {
    icon: "◉",
    title: "71 Core Modules",
    desc: "ISO9660, QCOW2, VHD, VMDK, WIM, FFU & more",
    color: theme.secondary,
  },
  {
    icon: "▤",
    title: "47 CLI Commands",
    desc: "Write, verify, clone, erase, checksum, boot...",
    color: theme.success,
  },
  {
    icon: "♫",
    title: "GUI + TUI + CLI",
    desc: "Three interfaces — one tool",
    color: theme.warning,
  },
  {
    icon: "✧",
    title: "951 Tests",
    desc: "Comprehensive test coverage across all modules",
    color: theme.accent,
  },
  {
    icon: "♨",
    title: "Blazing Fast",
    desc: "Async I/O, zero-copy, parallel decompression",
    color: theme.error,
  },
];

export const FeaturesScene: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Section title
  const titleOpacity = interpolate(frame, [0, 20], [0, 1], {
    extrapolateRight: "clamp",
  });
  const titleX = interpolate(frame, [0, 20], [-40, 0], {
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
      {/* Background accent line */}
      <div
        style={{
          position: "absolute",
          left: 60,
          top: 0,
          bottom: 0,
          width: 2,
          background: `linear-gradient(to bottom, transparent, ${theme.accent}40, transparent)`,
        }}
      />

      {/* Section title */}
      <div
        style={{
          opacity: titleOpacity,
          transform: `translateX(${titleX}px)`,
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
          //
        </span>
        Features
      </div>

      {/* Feature grid (2x3) */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          gap: 28,
          maxWidth: 1600,
        }}
      >
        {features.map((feat, i) => {
          const delay = 20 + i * 18;
          const cardScale = spring({
            frame: frame - delay,
            fps,
            config: { damping: 14, stiffness: 100 },
          });
          const cardOpacity = interpolate(frame, [delay, delay + 15], [0, 1], {
            extrapolateRight: "clamp",
          });

          return (
            <div
              key={feat.title}
              style={{
                opacity: cardOpacity,
                transform: `scale(${Math.max(0, cardScale)})`,
                backgroundColor: theme.bgCard,
                borderRadius: 12,
                padding: "28px 32px",
                border: `1px solid ${theme.border}`,
                boxShadow:
                  cardOpacity > 0.8 ? glowBox(feat.color, 10) : "none",
                display: "flex",
                alignItems: "center",
                gap: 24,
              }}
            >
              {/* Icon */}
              <div
                style={{
                  fontSize: 36,
                  width: 64,
                  height: 64,
                  borderRadius: 12,
                  backgroundColor: `${feat.color}15`,
                  border: `1px solid ${feat.color}40`,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  flexShrink: 0,
                  color: feat.color,
                  textShadow: glowText(feat.color, 8),
                }}
              >
                {feat.icon}
              </div>
              {/* Text */}
              <div>
                <div
                  style={{
                    fontSize: 22,
                    fontWeight: 700,
                    color: theme.textPrimary,
                    marginBottom: 6,
                  }}
                >
                  {feat.title}
                </div>
                <div
                  style={{
                    fontSize: 16,
                    color: theme.textSecondary,
                    lineHeight: 1.4,
                  }}
                >
                  {feat.desc}
                </div>
              </div>
            </div>
          );
        })}
      </div>

      <div style={scanlineOverlay} />
    </AbsoluteFill>
  );
};
