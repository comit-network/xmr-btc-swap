import {
  Typography,
  Box,
  Alert,
  DialogContent,
  DialogActions,
  Button,
} from "@mui/material";
import BitcoinQrCode from "renderer/components/modal/swap/BitcoinQrCode";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import SwapOverview from "../components/SwapOverview";

export default function GetBitcoin({ onNext }: { onNext: () => void }) {
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
          <Typography variant="h3">Get Bitcoin</Typography>
          <Typography variant="body1">
            Send Bitcoin to your internal wallet
          </Typography>
          <Box
            sx={{
              display: "flex",
              flexDirection: "row",
              gap: 2,
            }}
          >
            <Box
              sx={{
                display: "flex",
                flexDirection: "column",
                gap: 2,
                backgroundColor: "white",
                padding: 2,
                borderRadius: 2,
                maxWidth: "200px",
              }}
            >
              <BitcoinQrCode address="1234567890" />
            </Box>
            <Box
              sx={{
                display: "flex",
                flexDirection: "column",
                gap: 2,
              }}
            >
              <ActionableMonospaceTextBox content="1234567890" />
              <ActionableMonospaceTextBox content="1234567890" />
            </Box>
          </Box>
        </Box>
      </DialogContent>
      <DialogActions>
        <Button variant="contained" color="primary" onClick={onNext}>
          Next
        </Button>
      </DialogActions>
    </>
  );
}
