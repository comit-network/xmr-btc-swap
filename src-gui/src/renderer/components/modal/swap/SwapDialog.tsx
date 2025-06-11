import {
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
} from "@mui/material";
import { useState } from "react";
import { swapReset } from "store/features/swapSlice";
import { useAppDispatch, useAppSelector, useIsSwapRunning } from "store/hooks";
import SwapSuspendAlert from "../SwapSuspendAlert";
import DebugPage from "./pages/DebugPage";
import SwapStatePage from "./pages/SwapStatePage";
import SwapDialogTitle from "./SwapDialogTitle";
import SwapStateStepper from "./SwapStateStepper";

export default function SwapDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const swap = useAppSelector((state) => state.swap);
  const isSwapRunning = useIsSwapRunning();
  const [debug, setDebug] = useState(false);
  const [openSuspendAlert, setOpenSuspendAlert] = useState(false);

  const dispatch = useAppDispatch();

  function onCancel() {
    if (isSwapRunning) {
      setOpenSuspendAlert(true);
    } else {
      onClose();
      dispatch(swapReset());
    }
  }

  // This prevents an issue where the Dialog is shown for a split second without a present swap state
  if (!open) return null;

  return (
    <Dialog open={open} onClose={onCancel} maxWidth="md" fullWidth>
      <SwapDialogTitle
        debug={debug}
        setDebug={setDebug}
        title="Swap Bitcoin for Monero"
      />

      <DialogContent
        dividers
        sx={{
          minHeight: "25rem",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          flex: 1,
          gap: "1rem",
        }}
      >
        {debug ? (
          <DebugPage />
        ) : (
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              gap: 2,
              justifyContent: "space-between",
              flex: 1,
            }}
          >
            <SwapStatePage state={swap.state} />
            <SwapStateStepper state={swap.state} />
          </Box>
        )}
      </DialogContent>

      <DialogActions>
        <Button onClick={onCancel} variant="text">
          Cancel
        </Button>
        <Button
          color="primary"
          variant="contained"
          onClick={onCancel}
          disabled={isSwapRunning || swap.state === null}
        >
          Done
        </Button>
      </DialogActions>

      <SwapSuspendAlert
        open={openSuspendAlert}
        onClose={() => setOpenSuspendAlert(false)}
      />
    </Dialog>
  );
}
