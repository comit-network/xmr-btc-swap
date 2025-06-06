import { Box } from "@mui/material";
import { useSwapInfosSortedByDate } from "store/hooks";
import SwapStatusAlert from "./SwapStatusAlert/SwapStatusAlert";

export default function SwapTxLockAlertsBox() {
  // We specifically choose ALL swaps here
  // If a swap is in a state where an Alert is not needed (becaue no Bitcoin have been locked or because the swap has been completed)
  // the SwapStatusAlert component will not render an Alert
  const swaps = useSwapInfosSortedByDate();

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      {swaps.map((swap) => (
        <SwapStatusAlert key={swap.swap_id} swap={swap} isRunning={false} />
      ))}
    </Box>
  );
}
