import { Box, DialogContentText } from "@mui/material";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import MoneroTransactionInfoBox from "../../MoneroTransactionInfoBox";

export default function WaitingForXmrConfirmationsBeforeRedeemPage({
  xmr_lock_txid,
  xmr_lock_tx_confirmations,
  xmr_lock_tx_target_confirmations,
}: TauriSwapProgressEventContent<"WaitingForXmrConfirmationsBeforeRedeem">) {
  return (
    <Box>
      <DialogContentText>
        We are waiting for the Monero lock transaction to receive enough
        confirmations before we can sweep them to your address.
      </DialogContentText>

      <MoneroTransactionInfoBox
        title="Monero Lock Transaction"
        txId={xmr_lock_txid}
        additionalContent={
        additionalContent={
          `Confirmations: ${xmr_lock_tx_confirmations}/${xmr_lock_tx_target_confirmations}`
        }
        }
        loading
      />
    </Box>
  );
}
