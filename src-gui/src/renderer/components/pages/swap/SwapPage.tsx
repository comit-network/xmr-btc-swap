import { Box, makeStyles } from "@material-ui/core";
import ApiAlertsBox from "./ApiAlertsBox";
import SwapWidget from "./SwapWidget";
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useAppDispatch } from "../../../../store/hooks";
import {
  confirmationRequested,
  confirmationResolved,
  ConfirmationRequestPayload,
} from "../../../../store/features/rpcSlice";
import ConfirmationModal from "../../modal/ConfirmationModal";

const useStyles = makeStyles((theme) => ({
  outer: {
    display: "flex",
    width: "100%",
    flexDirection: "column",
    alignItems: "center",
    paddingBottom: theme.spacing(1),
    gap: theme.spacing(1),
  },
}));

export default function SwapPage() {
  const classes = useStyles();
  const dispatch = useAppDispatch();

  // Add useEffect hook for event listeners
  useEffect(() => {
    let unlistenConfirmationRequest: (() => void) | undefined;
    let unlistenConfirmationResolved: (() => void) | undefined;

    const setupListeners = async () => {
      try {
        unlistenConfirmationRequest = await listen<ConfirmationRequestPayload>(
          "confirmation_request",
          (event) => {
            console.log("Received confirmation_request:", event.payload);
            dispatch(confirmationRequested(event.payload));
          }
        );

        unlistenConfirmationResolved = await listen<{ request_id: string }>(
          "confirmation_resolved",
          (event) => {
            console.log("Received confirmation_resolved:", event.payload);
            dispatch(confirmationResolved({ requestId: event.payload.request_id }));
          }
        );
      } catch (error) {
        console.error("Failed to set up confirmation listeners:", error);
      }
    };

    setupListeners();

    // Cleanup function
    return () => {
      unlistenConfirmationRequest?.();
      unlistenConfirmationResolved?.();
    };
  }, [dispatch]);

  return (
    <Box className={classes.outer}>
      <ApiAlertsBox />
      <SwapWidget />
      <ConfirmationModal />
    </Box>
  );
}
