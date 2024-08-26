import { Box, DialogContentText } from "@material-ui/core";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { useActiveSwapInfo } from "store/hooks";
import FeedbackInfoBox from "../../../../pages/help/FeedbackInfoBox";
import BitcoinTransactionInfoBox from "../../BitcoinTransactionInfoBox";

export default function BitcoinRefundedPage({
  btc_refund_txid,
}: TauriSwapProgressEventContent<"BtcRefunded">) {
  // TODO: Reimplement this using Tauri
  const swap = useActiveSwapInfo();
  const additionalContent = swap
    ? `Refund address: ${swap.btc_refund_address}`
    : null;

  return (
    <Box>
      <DialogContentText>
        Unfortunately, the swap was not successful. However, rest assured that
        all your Bitcoin has been refunded to the specified address. The swap
        process is now complete, and you are free to exit the application.
      </DialogContentText>
      <Box
        style={{
          display: "flex",
          flexDirection: "column",
          gap: "0.5rem",
        }}
      >
        {
          // TODO: We should display the confirmation count here
        }
        <BitcoinTransactionInfoBox
          title="Bitcoin Refund Transaction"
          txId={btc_refund_txid}
          loading={false}
          additionalContent={additionalContent}
        />
        <FeedbackInfoBox />
      </Box>
    </Box>
  );
}
