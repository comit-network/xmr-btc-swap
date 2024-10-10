import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  makeStyles,
} from "@material-ui/core";
import { useState } from "react";
import { swapReset } from "store/features/swapSlice";
import { useAppDispatch, useAppSelector, useIsSwapRunning } from "store/hooks";
import SwapSuspendAlert from "../SwapSuspendAlert";
import DebugPage from "./pages/DebugPage";
import SwapStatePage from "./pages/SwapStatePage";
import SwapDialogTitle from "./SwapDialogTitle";
import SwapStateStepper from "./SwapStateStepper";

const useStyles = makeStyles({
  content: {
    minHeight: "25rem",
    display: "flex",
    flexDirection: "column",
    justifyContent: "space-between",
  },
});

export default function SwapDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const classes = useStyles();

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

      <DialogContent dividers className={classes.content}>
        {debug ? (
          <DebugPage />
        ) : (
          <>
            <SwapStatePage state={swap.state} />
            <SwapStateStepper state={swap.state} />
          </>
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
