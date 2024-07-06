import { Box, DialogContentText } from '@material-ui/core';
import { SwapStateBtcRefunded } from 'models/storeModel';
import { useActiveSwapInfo } from 'store/hooks';
import BitcoinTransactionInfoBox from '../../BitcoinTransactionInfoBox';
import FeedbackInfoBox from '../../../../pages/help/FeedbackInfoBox';

export default function BitcoinRefundedPage({
  state,
}: {
  state: SwapStateBtcRefunded | null;
}) {
  const swap = useActiveSwapInfo();
  const additionalContent = swap
    ? `Refund address: ${swap.btcRefundAddress}`
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
          display: 'flex',
          flexDirection: 'column',
          gap: '0.5rem',
        }}
      >
        {state && (
          <BitcoinTransactionInfoBox
            title="Bitcoin Refund Transaction"
            txId={state.bobBtcRefundTxId}
            loading={false}
            additionalContent={additionalContent}
          />
        )}
        <FeedbackInfoBox />
      </Box>
    </Box>
  );
}
