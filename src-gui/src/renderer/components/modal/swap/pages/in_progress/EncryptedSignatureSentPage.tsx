import SwapStatusAlert from "renderer/components/alert/SwapStatusAlert/SwapStatusAlert";
import CircularProgressWithSubtitle from "../../CircularProgressWithSubtitle";
import { useActiveSwapInfo, useSwapInfosSortedByDate } from "store/hooks";
import { Box } from "@mui/material";

export default function EncryptedSignatureSentPage() {
  const swap = useActiveSwapInfo();

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: "1rem" }}>
      <SwapStatusAlert
        swap={swap}
        isRunning={true}
        onlyShowIfUnusualAmountOfTimeHasPassed={true}
      />
      <Box
        sx={{
          minHeight: "10rem",
        }}
      >
        <CircularProgressWithSubtitle description="Waiting for them to redeem the Bitcoin" />
      </Box>
    </Box>
  );
}
