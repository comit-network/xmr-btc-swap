import { Box, Typography } from "@mui/material";
import { Alert } from "@mui/material";
import WithdrawWidget from "./WithdrawWidget";

export default function WalletPage() {
  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        gap: "1rem",
      }}
    >
      <Typography variant="h3">Wallet</Typography>
      <Alert severity="info">
        You do not have to deposit money before starting a swap. Instead, you
        will be greeted with a deposit address after you initiate one.
      </Alert>
      <WithdrawWidget />
    </Box>
  );
}
