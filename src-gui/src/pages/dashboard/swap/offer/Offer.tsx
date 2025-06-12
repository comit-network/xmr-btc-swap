import {
  DialogContent,
  DialogActions,
  Button,
  Typography,
  Box,
  Divider,
  IconButton,
} from "@mui/material";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import MakerOfferItem from "../components/MakerOfferItem";
import { useState } from "react";

export default function Offer({
  onBack,
  onNext,
}: {
  onBack: () => void;
  onNext: () => void;
}) {
  const [feeExpanded, setFeeExpanded] = useState(false);
  return (
    <>
      <DialogContent>
        <Typography variant="body1">
          Confirm your offer to start the swap
        </Typography>
        <MakerOfferItem />

        <Box
          sx={{
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            gap: 2,
            mt: 2,
          }}
        >
          <Typography variant="body1">You send</Typography>
          <Typography sx={{ textAlign: "right" }} variant="body1">
            0.00002 BTC
          </Typography>
          <Divider sx={{ gridColumn: "span 2" }} />

          <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
            <Typography variant="body1">Fee</Typography>
            <IconButton
              onClick={() => setFeeExpanded(!feeExpanded)}
              disableFocusRipple
            >
              {feeExpanded ? <ExpandLessIcon /> : <ExpandMoreIcon />}
            </IconButton>
          </Box>
          <Typography sx={{ textAlign: "right" }} variant="body1">
            0.00002 BTC
          </Typography>
          {feeExpanded && (
            <>
              <Typography variant="body2">Bitcoin Transaction Fee</Typography>
              <Typography sx={{ textAlign: "right" }} variant="body2">
                0.00002 BTC
              </Typography>

              <Typography variant="body2">Exchange Fee</Typography>
              <Typography sx={{ textAlign: "right" }} variant="body2">
                0.00002 BTC
              </Typography>

              <Typography variant="body2">Monero Transaction Fee</Typography>
              <Typography sx={{ textAlign: "right" }} variant="body2">
                0.00002 XMR
              </Typography>

              <Typography variant="body2">Developer Tax</Typography>
              <Typography sx={{ textAlign: "right" }} variant="body2">
                0.00002 XMR
              </Typography>
            </>
          )}
          <Divider sx={{ gridColumn: "span 2" }} />

          <Typography variant="body1">You receive</Typography>
          <Typography sx={{ textAlign: "right" }} variant="body1">
            0.00002 XMR
          </Typography>
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
