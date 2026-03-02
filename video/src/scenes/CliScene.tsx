import React from "react";
import {
  AbsoluteFill,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { theme, fonts, glowText, scanlineOverlay } from "../theme";

// ─── Scene 3: CLI demo ────────────────────────────────────────────────────
// Frames 0–180 (6 seconds)

const termLines = [
  { text: "$ abt list", color: theme.accent, delay: 10 },
  { text: "  ◉ /dev/sdb  SanDisk Ultra  32 GiB  USB", color: theme.textSecondary, delay: 30 },
  { text: "  ○ /dev/sdc  Kingston DT    16 GiB  USB", color: theme.textSecondary, delay: 38 },
  { text: "", color: "transparent", delay: 0 },
  { text: "$ abt write ubuntu-24.04.iso /dev/sdb --verify", color: theme.accent, delay: 55 },
  { text: "  ⚡ Writing ubuntu-24.04.iso → /dev/sdb", color: theme.warning, delay: 75 },
  { text: "  ████████████████████░░░░░  78.3%  42.1 MiB/s  ETA 12s", color: theme.success, delay: 90 },
  { text: "  ████████████████████████  100.0%  Done!", color: theme.success, delay: 120 },
  { text: "  ✓ Verification passed — device safe to remove", color: theme.success, delay: 140 },
];

export const CliScene: React.FC = () => {
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
          marginBottom: 40,
          display: "flex",
          alignItems: "center",
          gap: 16,
        }}
      >
        <span
          style={{
            color: theme.accent,
            textShadow: glowText(theme.accent, 10),
          }}
        >
          {">_"}
        </span>
        Command Line
      </div>

      {/* Terminal window */}
      <div
        style={{
          backgroundColor: "#0D0D14",
          borderRadius: 14,
          border: `1px solid ${theme.border}`,
          overflow: "hidden",
          boxShadow: `0 8px 40px rgba(0,0,0,0.5), 0 0 20px ${theme.accent}10`,
          maxWidth: 1400,
        }}
      >
        {/* Title bar */}
        <div
          style={{
            padding: "12px 20px",
            backgroundColor: theme.bgCard,
            borderBottom: `1px solid ${theme.border}`,
            display: "flex",
            alignItems: "center",
            gap: 10,
          }}
        >
          <div
            style={{
              width: 14,
              height: 14,
              borderRadius: "50%",
              backgroundColor: theme.error,
            }}
          />
          <div
            style={{
              width: 14,
              height: 14,
              borderRadius: "50%",
              backgroundColor: theme.warning,
            }}
          />
          <div
            style={{
              width: 14,
              height: 14,
              borderRadius: "50%",
              backgroundColor: theme.success,
            }}
          />
          <span
            style={{
              color: theme.textMuted,
              fontSize: 14,
              marginLeft: 12,
              fontFamily: fonts.mono,
            }}
          >
            abt — bash
          </span>
        </div>

        {/* Terminal body */}
        <div
          style={{
            padding: "24px 28px",
            fontFamily: fonts.mono,
            fontSize: 20,
            lineHeight: 1.8,
          }}
        >
          {termLines.map((line, i) => {
            const lineOpacity = interpolate(
              frame,
              [line.delay, line.delay + 10],
              [0, 1],
              { extrapolateLeft: "clamp", extrapolateRight: "clamp" }
            );
            const lineX = interpolate(
              frame,
              [line.delay, line.delay + 10],
              [12, 0],
              { extrapolateLeft: "clamp", extrapolateRight: "clamp" }
            );

            // Typing cursor on current line
            const isCurrentLine =
              i < termLines.length - 1
                ? frame >= line.delay && frame < termLines[i + 1].delay
                : frame >= line.delay;

            return (
              <div
                key={i}
                style={{
                  opacity: lineOpacity,
                  transform: `translateX(${lineX}px)`,
                  color: line.color,
                  minHeight: line.text === "" ? 12 : undefined,
                  whiteSpace: "pre",
                }}
              >
                {line.text}
                {isCurrentLine && line.text.startsWith("$") && (
                  <span
                    style={{
                      opacity: Math.sin(frame * 0.3) > 0 ? 1 : 0,
                      color: theme.accent,
                      textShadow: glowText(theme.accent, 6),
                    }}
                  >
                    ▊
                  </span>
                )}
              </div>
            );
          })}
        </div>
      </div>

      <div style={scanlineOverlay} />
    </AbsoluteFill>
  );
};
