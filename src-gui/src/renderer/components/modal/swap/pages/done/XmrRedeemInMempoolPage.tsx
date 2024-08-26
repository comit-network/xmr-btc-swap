import { Box, DialogContentText } from "@material-ui/core";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import FeedbackInfoBox from "../../../../pages/help/FeedbackInfoBox";
import MoneroTransactionInfoBox from "../../MoneroTransactionInfoBox";

export default function XmrRedeemInMempoolPage({
  xmr_redeem_address,
  xmr_redeem_txid,
}: TauriSwapProgressEventContent<"XmrRedeemInMempool">) {
  // TODO: Reimplement this using Tauri
  //const additionalContent = swap
  //  ? `This transaction transfers ${getSwapXmrAmount(swap).toFixed(6)} XMR to ${
  //      state?.bobXmrRedeemAddress
  //    }`
  //  : null;

  return (
    <Box>
      <DialogContentText>
        The swap was successful and the Monero has been sent to the address you
        specified. The swap is completed and you may exit the application now.
      </DialogContentText>
      <Box
        style={{
          display: "flex",
          flexDirection: "column",
          gap: "0.5rem",
        }}
      >
        <MoneroTransactionInfoBox
          title="Monero Redeem Transaction"
          txId={xmr_redeem_txid}
          additionalContent={`The funds have been sent to the address ${xmr_redeem_address}`}
          loading={false}
        />
        <FeedbackInfoBox />
      </Box>
    </Box>
  );
}
