import React from "react";
import {
  AbsoluteFill,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { theme, fonts, glowText, glowBox, scanlineOverlay } from "../theme";

// ─── Scene 4: Format support ─────────────────────────────────────────────
// Frames 0–180 (6 seconds)

const formatGroups = [
  {
    label: "Disk Images",
    formats: ["ISO", "IMG", "RAW", "DD", "BIN", "DMG"],
    color: theme.accent,
  },
  {
    label: "Virtual Disks",
    formats: ["VHD", "VHDX", "VMDK", "QCOW2"],
    color: theme.secondary,
  },
  {
    label: "Windows",
    formats: ["WIM", "FFU", "ESD"],
    color: theme.warning,
  },
  {
    label: "Compression",
    formats: ["GZ", "BZ2", "XZ", "ZST", "ZIP"],
    color: theme.success,
  },
];

export const FormatsScene: React.FC = () => {
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
        <span
          style={{
            color: theme.accent,
            textShadow: glowText(theme.accent, 10),
          }}
        >
          {"{ }"}
        </span>
        Format Support
      </div>

      {/* Format groups */}
      <div style={{ display: "flex", flexDirection: "column", gap: 36 }}>
        {formatGroups.map((group, gi) => {
          const groupDelay = 15 + gi * 25;
          const groupOpacity = interpolate(
            frame,
            [groupDelay, groupDelay + 15],
            [0, 1],
            { extrapolateRight: "clamp" }
          );
          const groupX = interpolate(
            frame,
            [groupDelay, groupDelay + 15],
            [-30, 0],
            { extrapolateRight: "clamp" }
          );

          return (
            <div
              key={group.label}
              style={{
                opacity: groupOpacity,
                transform: `translateX(${groupX}px)`,
              }}
            >
              {/* Group label */}
              <div
                style={{
                  fontSize: 18,
                  fontWeight: 600,
                  color: group.color,
                  marginBottom: 14,
                  letterSpacing: 2,
                  textTransform: "uppercase",
                  textShadow: glowText(group.color, 6),
                }}
              >
                {group.label}
              </div>

              {/* Format badges */}
              <div style={{ display: "flex", gap: 14, flexWrap: "wrap" }}>
                {group.formats.map((fmt, fi) => {
                  const badgeDelay = groupDelay + 10 + fi * 5;
                  const badgeScale = spring({
                    frame: frame - badgeDelay,
                    fps,
                    config: { damping: 12, stiffness: 120 },
                  });

                  return (
                    <div
                      key={fmt}
                      style={{
                        transform: `scale(${Math.max(0, badgeScale)})`,
                        padding: "10px 24px",
                        borderRadius: 8,
                        backgroundColor: `${group.color}12`,
                        border: `1px solid ${group.color}50`,
                        color: group.color,
                        fontSize: 18,
                        fontWeight: 700,
                        fontFamily: fonts.mono,
                        letterSpacing: 1,
                        boxShadow:
                          badgeScale > 0.8
                            ? `0 0 12px ${group.color}30`
                            : "none",
                      }}
                    >
                      {fmt}
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>

      <div style={scanlineOverlay} />
    </AbsoluteFill>
  );
};
