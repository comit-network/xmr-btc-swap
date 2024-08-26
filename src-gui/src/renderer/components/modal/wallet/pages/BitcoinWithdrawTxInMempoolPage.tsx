import { DialogContentText } from "@material-ui/core";
import BitcoinTransactionInfoBox from "../../swap/BitcoinTransactionInfoBox";

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
