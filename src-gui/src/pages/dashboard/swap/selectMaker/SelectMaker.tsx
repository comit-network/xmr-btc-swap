import {
  Typography,
  Box,
  DialogContent,
  DialogActions,
  Button,
} from "@mui/material";
import MakerOfferItem from "../components/MakerOfferItem";
import SwapOverview from "../components/SwapOverview";

export default function SelectMaker({
  onNext,
  onBack,
}: {
  onNext: () => void;
  onBack: () => void;
}) {
  return (
    <>
      <DialogContent>
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            gap: 2,
          }}
        >
          <SwapOverview />
          <Box
            sx={{
              display: "flex",
              flexDirection: "row",
              gap: 1,
            }}
          >
            <Typography variant="h3">Select a Maker</Typography>
            <Button variant="text">Connect to Rendezvous Point</Button>
          </Box>
          <Typography variant="body1">Best offer</Typography>
          <MakerOfferItem />
          <Typography variant="body1">Other offers</Typography>
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              gap: 1,
            }}
          >
            <MakerOfferItem />
            <MakerOfferItem />
          </Box>
        </Box>
      </DialogContent>
      <DialogActions>
        <Button variant="outlined" onClick={onBack}>
          Back
        </Button>
        <Button variant="contained" color="primary" onClick={onNext}>
          Get Offer
        </Button>
      </DialogActions>
    </>
  );
}
