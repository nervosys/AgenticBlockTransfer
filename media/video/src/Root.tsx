import { Composition } from "remotion";
import { AbtVideo } from "./AbtVideo";

export const RemotionRoot: React.FC = () => {
  return (
    <>
      <Composition
        id="AbtVideo"
        component={AbtVideo}
        durationInFrames={900}
        fps={30}
        width={1920}
        height={1080}
      />
    </>
  );
};
