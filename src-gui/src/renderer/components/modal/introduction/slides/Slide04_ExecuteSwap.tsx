import { Typography } from "@mui/material";
import SlideTemplate from "./SlideTemplate";
import imagePath from "assets/simpleSwapFlowDiagram.svg";

export default function Slide02_ChooseAMaker(props: slideProps) {
  return (
    <SlideTemplate
      title="Execute Swap"
      stepLabel="Step 3"
      {...props}
      imagePath={imagePath}
    >
      <Typography variant="subtitle1">After confirming:</Typography>
      <Typography>
        <ol>
          <li>Your Bitcoin are locked</li>
          <li>Maker locks the Monero</li>
          <li>Maker reedems the Bitcoin</li>
          <li>Monero is sent to your address</li>
        </ol>
      </Typography>
    </SlideTemplate>
  );
}
