import { Box, DialogContentText } from "@mui/material";
import FeedbackInfoBox from "../../../../pages/help/FeedbackInfoBox";
import { TauriSwapProgressEventExt } from "models/tauriModelExt";

export default function BitcoinPunishedPage({
  state,
}: {
  state:
    | TauriSwapProgressEventExt<"BtcPunished">
    | TauriSwapProgressEventExt<"CooperativeRedeemRejected">;
}) {
  return (
    <Box>
      <DialogContentText>
        Unfortunately, the swap was unsuccessful. Since you did not refund in
        time, the Bitcoin has been lost. However, with the cooperation of the
        other party, you might still be able to redeem the Monero, although this
        is not guaranteed.{" "}
        {state.type === "CooperativeRedeemRejected" && (
          <>
            <br />
            We tried to redeem the Monero with the other party's help, but it
            was unsuccessful (reason: {state.content.reason}). Attempting again
            at a later time might yield success. <br />
          </>
        )}
      </DialogContentText>
      <FeedbackInfoBox />
    </Box>
  );
}
