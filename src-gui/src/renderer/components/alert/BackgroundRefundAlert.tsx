import { BackgroundRefundState } from "models/tauriModel";
import { useAppSelector } from "store/hooks";
import { LoadingSpinnerAlert } from "./LoadingSpinnerAlert";
import { AlertTitle } from "@mui/material";
import TruncatedText from "../other/TruncatedText";
import { useSnackbar } from "notistack";
import { useEffect } from "react";

export default function BackgroundRefundAlert() {
  const backgroundRefund = useAppSelector(
    (state) => state.rpc.state.backgroundRefund,
  );
  const notistack = useSnackbar();

  useEffect(() => {
    // If we failed to refund, show a notification
    if (backgroundRefund?.state.type === "Failed") {
      notistack.enqueueSnackbar(
        <>
          Our attempt to refund {backgroundRefund.swapId} in the background
          failed.
          <br />
          Error: {backgroundRefund.state.content.error}
        </>,
        { variant: "error", autoHideDuration: 60 * 1000 },
      );
    }

    // If we successfully refunded, show a notification as well
    if (backgroundRefund?.state.type === "Completed") {
      notistack.enqueueSnackbar(
        `The swap ${backgroundRefund.swapId} has been refunded in the background.`,
        { variant: "success", persist: true },
      );
    }
  }, [backgroundRefund]);

  if (backgroundRefund?.state.type === "Started") {
    return (
      <LoadingSpinnerAlert>
        <AlertTitle>Refund in progress</AlertTitle>
        The swap <TruncatedText>{backgroundRefund.swapId}</TruncatedText> is
        being refunded in the background.
      </LoadingSpinnerAlert>
    );
  }

  return null;
}
