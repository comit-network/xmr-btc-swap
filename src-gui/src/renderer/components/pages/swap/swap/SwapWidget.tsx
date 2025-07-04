import { Box, Button, Dialog, DialogActions, Paper } from "@mui/material";
import { useActiveSwapInfo, useAppSelector } from "store/hooks";
import SwapStatePage from "renderer/components/pages/swap/swap/SwapStatePage";
import CancelButton from "./CancelButton";
import SwapStateStepper from "renderer/components/modal/swap/SwapStateStepper";
import SwapStatusAlert from "renderer/components/alert/SwapStatusAlert/SwapStatusAlert";
import DebugPageSwitchBadge from "renderer/components/modal/swap/pages/DebugPageSwitchBadge";
import DebugPage from "renderer/components/modal/swap/pages/DebugPage";
import { useState } from "react";

export default function SwapWidget() {
  const swap = useAppSelector((state) => state.swap);
  const swapInfo = useActiveSwapInfo();

  const [debug, setDebug] = useState(false);

  return (
    <Box
      sx={{ display: "flex", flexDirection: "column", gap: 2, width: "100%" }}
    >
      <SwapStatusAlert swap={swapInfo} onlyShowIfUnusualAmountOfTimeHasPassed />
      <Dialog
        fullWidth
        maxWidth="md"
        open={debug}
        onClose={() => setDebug(false)}
      >
        <DebugPage />
        <DialogActions>
          <Button variant="outlined" onClick={() => setDebug(false)}>
            Close
          </Button>
        </DialogActions>
      </Dialog>
      <Paper
        elevation={3}
        sx={{
          width: "100%",
          maxWidth: 800,
          borderRadius: 2,
          margin: "0 auto",
          padding: 2,
          display: "flex",
          flexDirection: "column",
          gap: 2,
          justifyContent: "space-between",
          flex: 1,
        }}
      >
        <SwapStatePage state={swap.state} />
        {swap.state !== null && (
          <>
            <SwapStateStepper state={swap.state} />
            <Box
              sx={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
              }}
            >
              <CancelButton />
              <DebugPageSwitchBadge enabled={debug} setEnabled={setDebug} />
            </Box>
          </>
        )}
      </Paper>
    </Box>
  );
}
