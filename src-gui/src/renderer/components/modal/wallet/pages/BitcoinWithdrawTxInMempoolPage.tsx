import { Button, DialogActions, DialogContentText } from '@material-ui/core';
import BitcoinTransactionInfoBox from '../../swap/BitcoinTransactionInfoBox';
import WithdrawDialogContent from '../WithdrawDialogContent';

export default function BtcTxInMempoolPageContent({
  withdrawTxId,
  onCancel,
}: {
  withdrawTxId: string;
  onCancel: () => void;
}) {
  return (
    <>
      <WithdrawDialogContent>
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
      </WithdrawDialogContent>
      <DialogActions>
        <Button onClick={onCancel} variant="text">
          Cancel
        </Button>
        <Button onClick={onCancel} color="primary" variant="contained">
          Done
        </Button>
      </DialogActions>
    </>
  );
}
