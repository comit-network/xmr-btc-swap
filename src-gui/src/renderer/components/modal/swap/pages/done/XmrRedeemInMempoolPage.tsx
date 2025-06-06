import { Box, DialogContentText } from "@mui/material";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import FeedbackInfoBox from "../../../../pages/help/FeedbackInfoBox";
import MoneroTransactionInfoBox from "../../MoneroTransactionInfoBox";

export default function XmrRedeemInMempoolPage(
  state: TauriSwapProgressEventContent<"XmrRedeemInMempool">,
) {
  const xmr_redeem_txid = state.xmr_redeem_txids[0] ?? null;

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
          additionalContent={`The funds have been sent to the address ${state.xmr_redeem_address}`}
          loading={false}
        />
        <FeedbackInfoBox />
      </Box>
    </Box>
  );
}
