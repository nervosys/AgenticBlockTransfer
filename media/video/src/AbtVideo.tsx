import React from "react";
import {
  AbsoluteFill,
  Sequence,
  interpolate,
  useCurrentFrame,
} from "remotion";
import { IntroScene } from "./scenes/IntroScene";
import { FeaturesScene } from "./scenes/FeaturesScene";
import { CliScene } from "./scenes/CliScene";
import { FormatsScene } from "./scenes/FormatsScene";
import { ArchScene } from "./scenes/ArchScene";
import { OutroScene } from "./scenes/OutroScene";
import { theme } from "./theme";

// ─── Transition wrapper ──────────────────────────────────────────────────
const FadeTransition: React.FC<{
  children: React.ReactNode;
  durationInFrames: number;
  fadeIn?: number;
  fadeOut?: number;
}> = ({ children, durationInFrames, fadeIn = 15, fadeOut = 15 }) => {
  const frame = useCurrentFrame();

  // Ensure inputRange is strictly monotonically increasing
  const safeFadeIn = Math.max(1, fadeIn);
  const safeFadeOut = Math.max(1, fadeOut);

  const opacity = interpolate(
    frame,
    [0, safeFadeIn, durationInFrames - safeFadeOut, durationInFrames],
    [0, 1, 1, 0],
    { extrapolateLeft: "clamp", extrapolateRight: "clamp" }
  );

  return (
    <AbsoluteFill style={{ opacity }}>
      {children}
    </AbsoluteFill>
  );
};

// ─── Main composition ────────────────────────────────────────────────────
// Total: 900 frames = 30 seconds at 30fps
//
// Scene breakdown:
//   Intro:    0–149   (5s)
//   Features: 135–389 (8.5s, 15f overlap)
//   CLI:      375–569 (6.5s, 15f overlap)
//   Formats:  555–749 (6.5s, 15f overlap)
//   Arch:     735–869 (4.5s, 15f overlap)  — shortened
//   Outro:    855–899 (4.5s, 15f overlap)  — shortened

export const AbtVideo: React.FC = () => {
  return (
    <AbsoluteFill style={{ backgroundColor: theme.bgDeep }}>
      {/* Scene 1: Intro */}
      <Sequence from={0} durationInFrames={150}>
        <FadeTransition durationInFrames={150} fadeIn={0} fadeOut={15}>
          <IntroScene />
        </FadeTransition>
      </Sequence>

      {/* Scene 2: Features */}
      <Sequence from={135} durationInFrames={255}>
        <FadeTransition durationInFrames={255}>
          <FeaturesScene />
        </FadeTransition>
      </Sequence>

      {/* Scene 3: CLI demo */}
      <Sequence from={375} durationInFrames={195}>
        <FadeTransition durationInFrames={195}>
          <CliScene />
        </FadeTransition>
      </Sequence>

      {/* Scene 4: Formats */}
      <Sequence from={555} durationInFrames={195}>
        <FadeTransition durationInFrames={195}>
          <FormatsScene />
        </FadeTransition>
      </Sequence>

      {/* Scene 5: Architecture */}
      <Sequence from={720} durationInFrames={135}>
        <FadeTransition durationInFrames={135}>
          <ArchScene />
        </FadeTransition>
      </Sequence>

      {/* Scene 6: Outro */}
      <Sequence from={810} durationInFrames={90}>
        <FadeTransition durationInFrames={90} fadeOut={20}>
          <OutroScene />
        </FadeTransition>
      </Sequence>
    </AbsoluteFill>
  );
};
