import { Typography } from "@mui/material";
import SlideTemplate from "./SlideTemplate";
import imagePath from "assets/mockConfigureSwap.svg";

export default function Slide02_ChooseAMaker(props: slideProps) {
  return (
    <SlideTemplate
      title="Prepare Swap"
      stepLabel="Step 2"
      {...props}
      imagePath={imagePath}
    >
      <Typography variant="subtitle1">
        To initiate a swap, provide a Monero address and optionally a Bitcoin
        refund address.
      </Typography>
    </SlideTemplate>
  );
}
