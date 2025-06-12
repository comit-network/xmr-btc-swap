import { Alert, Box, Typography } from "@mui/material";
import SwapAmountSelector from "./SwapAmountSelector";
import ReceiveAddressSelector from "./ReceiveAddressSelector";
import MissingBtcAlert from "./MissingBtcAlert";

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
      <MissingBtcAlert />
    </Box>
  );
}
