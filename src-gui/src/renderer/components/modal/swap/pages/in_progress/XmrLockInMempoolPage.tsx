import { Box, DialogContentText } from '@material-ui/core';
import { SwapStateXmrLockInMempool } from 'models/storeModel';
import MoneroTransactionInfoBox from '../../MoneroTransactionInfoBox';

type XmrLockTxInMempoolPageProps = {
  state: SwapStateXmrLockInMempool;
};

export default function XmrLockTxInMempoolPage({
  state,
}: XmrLockTxInMempoolPageProps) {
  const additionalContent = `Confirmations: ${state.aliceXmrLockTxConfirmations}/10`;

  return (
    <Box>
      <DialogContentText>
        They have published their Monero lock transaction. The swap will proceed
        once the transaction has been confirmed.
      </DialogContentText>

      <MoneroTransactionInfoBox
        title="Monero Lock Transaction"
        txId={state.aliceXmrLockTxId}
        additionalContent={additionalContent}
        loading
      />
    </Box>
  );
}
