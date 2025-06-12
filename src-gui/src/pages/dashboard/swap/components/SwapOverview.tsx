import { Alert, Box, Typography } from "@mui/material";
import SwapAmountSelector from "./SwapAmountSelector";
import ReceiveAddressSelector from "./ReceiveAddressSelector";

export default function SwapOverview() {
  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        gap: 2,
      }}
    >
      <ReceiveAddressSelector />
      <SwapAmountSelector fullWidth />
      <Alert severity="info" variant="outlined">
        Your Wallet has 0.00000000 BTC. You need an additional 0.00000000 BTC to
        swap your desired XMR amount.
      </Alert>
    </Box>
  );
}
