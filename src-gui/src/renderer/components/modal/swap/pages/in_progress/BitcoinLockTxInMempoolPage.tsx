import { Box, DialogContentText } from '@material-ui/core';
import { SwapStateBtcLockInMempool } from 'models/storeModel';
import BitcoinTransactionInfoBox from '../../BitcoinTransactionInfoBox';
import SwapMightBeCancelledAlert from '../../../../alert/SwapMightBeCancelledAlert';

type BitcoinLockTxInMempoolPageProps = {
  state: SwapStateBtcLockInMempool;
};

export default function BitcoinLockTxInMempoolPage({
  state,
}: BitcoinLockTxInMempoolPageProps) {
  return (
    <Box>
      <SwapMightBeCancelledAlert
        bobBtcLockTxConfirmations={state.bobBtcLockTxConfirmations}
      />
      <DialogContentText>
        The Bitcoin lock transaction has been published. The swap will proceed
        once the transaction is confirmed and the swap provider locks their
        Monero.
      </DialogContentText>
      <BitcoinTransactionInfoBox
        title="Bitcoin Lock Transaction"
        txId={state.bobBtcLockTxId}
        loading
        additionalContent={
          <>
            Most swap providers require one confirmation before locking their
            Monero
            <br />
            Confirmations: {state.bobBtcLockTxConfirmations}
          </>
        }
      />
    </Box>
  );
}
