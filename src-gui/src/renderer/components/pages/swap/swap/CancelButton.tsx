import { Box, Button } from "@mui/material";
import { haveFundsBeenLocked } from "models/tauriModelExt";
import { getCurrentSwapId, suspendCurrentSwap } from "renderer/rpc";
import { swapReset } from "store/features/swapSlice";
import { useAppDispatch, useAppSelector, useIsSwapRunning } from "store/hooks";
import { useState } from "react";
import SwapSuspendAlert from "renderer/components/modal/SwapSuspendAlert";

export default function CancelButton() {
  const dispatch = useAppDispatch();
  const swap = useAppSelector((state) => state.swap);
  const isSwapRunning = useIsSwapRunning();
  const [openSuspendAlert, setOpenSuspendAlert] = useState(false);

  const hasFundsBeenLocked = haveFundsBeenLocked(swap.state?.curr);

  async function onCancel() {
    const swapId = await getCurrentSwapId();

    if (swapId.swap_id !== null) {
      if (hasFundsBeenLocked && isSwapRunning) {
        setOpenSuspendAlert(true);
        return;
      }

      await suspendCurrentSwap();
    }

    dispatch(swapReset());
  }

  return (
    <>
      <SwapSuspendAlert
        open={openSuspendAlert}
        onClose={() => setOpenSuspendAlert(false)}
      />
      <Box
        sx={{ display: "flex", justifyContent: "flex-start", width: "100%" }}
      >
        <Button variant="outlined" onClick={onCancel}>
          {hasFundsBeenLocked && swap.state?.curr.type !== "Released"
            ? "Suspend"
            : swap.state?.curr.type === "Released"
              ? "Close"
              : "Cancel"}
        </Button>
      </Box>
    </>
  );
}
