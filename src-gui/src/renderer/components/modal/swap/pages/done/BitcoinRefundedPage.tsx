import { Box, DialogContentText } from "@mui/material";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { useActiveSwapInfo } from "store/hooks";
import FeedbackInfoBox from "../../../../pages/help/FeedbackInfoBox";
import BitcoinTransactionInfoBox from "../../BitcoinTransactionInfoBox";

export function BitcoinRefundPublishedPage({
  btc_refund_txid,
}: TauriSwapProgressEventContent<"BtcRefundPublished">) {
  return (
    <MultiBitcoinRefundedPage
      btc_refund_txid={btc_refund_txid}
      btc_refund_finalized={false}
    />
  );
}

export function BitcoinEarlyRefundPublishedPage({
  btc_early_refund_txid,
}: TauriSwapProgressEventContent<"BtcEarlyRefundPublished">) {
  return (
    <MultiBitcoinRefundedPage
      btc_refund_txid={btc_early_refund_txid}
      btc_refund_finalized={false}
    />
  );
}

export function BitcoinRefundedPage({
  btc_refund_txid,
}: TauriSwapProgressEventContent<"BtcRefunded">) {
  return (
    <MultiBitcoinRefundedPage
      btc_refund_txid={btc_refund_txid}
      btc_refund_finalized={true}
    />
  );
}

export function BitcoinEarlyRefundedPage({
  btc_early_refund_txid,
}: TauriSwapProgressEventContent<"BtcEarlyRefunded">) {
  return (
    <MultiBitcoinRefundedPage
      btc_refund_txid={btc_early_refund_txid}
      btc_refund_finalized={true}
    />
  );
}

function MultiBitcoinRefundedPage({
  btc_refund_txid,
  btc_refund_finalized,
}: {
  btc_refund_txid: string;
  btc_refund_finalized: boolean;
}) {
  const swap = useActiveSwapInfo();
  const additionalContent = swap ? (
    <>
      {!btc_refund_finalized &&
        "Waiting for refund transaction to be confirmed"}
      {!btc_refund_finalized && <br />}
      Refund address: {swap.btc_refund_address}
    </>
  ) : null;

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
        <BitcoinTransactionInfoBox
          title="Bitcoin Refund Transaction"
          txId={btc_refund_txid}
          loading={!btc_refund_finalized}
          additionalContent={additionalContent}
        />
        <FeedbackInfoBox />
      </Box>
    </Box>
  );
}
