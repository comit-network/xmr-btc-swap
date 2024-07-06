import { useActiveSwapInfo } from 'store/hooks';
import { SwapStateName } from 'models/rpcModel';
import {
  isSwapStateBtcPunished,
  isSwapStateBtcRefunded,
  isSwapStateXmrRedeemInMempool,
  SwapStateProcessExited,
} from '../../../../../../models/storeModel';
import XmrRedeemInMempoolPage from '../done/XmrRedeemInMempoolPage';
import BitcoinPunishedPage from '../done/BitcoinPunishedPage';
// eslint-disable-next-line import/no-cycle
import SwapStatePage from '../SwapStatePage';
import BitcoinRefundedPage from '../done/BitcoinRefundedPage';
import ProcessExitedAndNotDonePage from './ProcessExitedAndNotDonePage';

type ProcessExitedPageProps = {
  state: SwapStateProcessExited;
};

export default function ProcessExitedPage({ state }: ProcessExitedPageProps) {
  const swap = useActiveSwapInfo();

  // If we have a swap state, for a "done" state we should use it to display additional information that can't be extracted from the database
  if (
    isSwapStateXmrRedeemInMempool(state.prevState) ||
    isSwapStateBtcRefunded(state.prevState) ||
    isSwapStateBtcPunished(state.prevState)
  ) {
    return <SwapStatePage swapState={state.prevState} />;
  }

  // If we don't have a swap state for a "done" state, we should fall back to using the database to display as much information as we can
  if (swap) {
    if (swap.stateName === SwapStateName.XmrRedeemed) {
      return <XmrRedeemInMempoolPage state={null} />;
    }
    if (swap.stateName === SwapStateName.BtcRefunded) {
      return <BitcoinRefundedPage state={null} />;
    }
    if (swap.stateName === SwapStateName.BtcPunished) {
      return <BitcoinPunishedPage />;
    }
  }

  // If the swap is not a "done" state (or we don't have a db state because the swap did complete the SwapSetup yet) we should tell the user and show logs
  return <ProcessExitedAndNotDonePage state={state} />;
}
