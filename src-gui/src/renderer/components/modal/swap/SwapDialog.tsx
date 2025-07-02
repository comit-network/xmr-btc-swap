import { Box, Dialog, DialogActions, DialogContent } from "@mui/material";
import { useState } from "react";
import { useAppSelector } from "store/hooks";
import DebugPage from "./pages/DebugPage";
import SwapStatePage from "renderer/components/pages/swap/swap/SwapStatePage";
import SwapDialogTitle from "./SwapDialogTitle";
import SwapStateStepper from "./SwapStateStepper";
import CancelButton from "renderer/components/pages/swap/swap/CancelButton";

export default function SwapDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const swap = useAppSelector((state) => state.swap);
  const [debug, setDebug] = useState(false);

  // This prevents an issue where the Dialog is shown for a split second without a present swap state
  if (!open) return null;

  return (
    <Dialog open={open} onClose={onClose} maxWidth="md" fullWidth>
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
        <CancelButton />
      </DialogActions>
    </Dialog>
  );
}
