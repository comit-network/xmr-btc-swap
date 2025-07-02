import { DialogContentText } from "@mui/material";
import BitcoinTransactionInfoBox from "renderer/components/pages/swap/swap/components/BitcoinTransactionInfoBox";

export default function BtcTxInMempoolPageContent({
  withdrawTxId,
}: {
  withdrawTxId: string;
}) {
  return (
    <>
      <DialogContentText>
        All funds of the internal Bitcoin wallet have been transferred to your
        withdraw address.
      </DialogContentText>
      <BitcoinTransactionInfoBox
        txId={withdrawTxId}
        loading={false}
        title="Bitcoin Withdraw Transaction"
        additionalContent={null}
      />
    </>
  );
}
