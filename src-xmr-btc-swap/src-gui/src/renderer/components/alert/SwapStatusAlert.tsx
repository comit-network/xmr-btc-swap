import { Alert, AlertTitle } from '@material-ui/lab/';
import { Box, makeStyles } from '@material-ui/core';
import { ReactNode } from 'react';
import { exhaustiveGuard } from 'utils/typescriptUtils';
import {
  SwapCancelRefundButton,
  SwapResumeButton,
} from '../pages/history/table/HistoryRowActions';
import HumanizedBitcoinBlockDuration from '../other/HumanizedBitcoinBlockDuration';
import {
  GetSwapInfoResponse,
  GetSwapInfoResponseRunningSwap,
  isGetSwapInfoResponseRunningSwap,
  isSwapTimelockInfoCancelled,
  isSwapTimelockInfoNone,
  isSwapTimelockInfoPunished,
  SwapStateName,
  SwapTimelockInfoCancelled,
  SwapTimelockInfoNone,
} from '../../../models/rpcModel';
import { SwapMoneroRecoveryButton } from '../pages/history/table/SwapMoneroRecoveryButton';

const useStyles = makeStyles({
  box: {
    display: 'flex',
    flexDirection: 'column',
    gap: '0.5rem',
  },
  list: {
    padding: '0px',
    margin: '0px',
  },
});

/**
 * Component for displaying a list of messages.
 * @param messages - Array of messages to display.
 * @returns JSX.Element
 */
const MessageList = ({ messages }: { messages: ReactNode[] }) => {
  const classes = useStyles();
  return (
    <ul className={classes.list}>
      {messages.map((msg, i) => (
        // eslint-disable-next-line react/no-array-index-key
        <li key={i}>{msg}</li>
      ))}
    </ul>
  );
};

/**
 * Sub-component for displaying alerts when the swap is in a safe state.
 * @param swap - The swap information.
 * @returns JSX.Element
 */
const BitcoinRedeemedStateAlert = ({ swap }: { swap: GetSwapInfoResponse }) => {
  const classes = useStyles();
  return (
    <Box className={classes.box}>
      <MessageList
        messages={[
          'The Bitcoin has been redeemed by the other party',
          'There is no risk of losing funds. You can take your time',
          'The Monero will be automatically redeemed to the address you provided as soon as you resume the swap',
          'If this step fails, you can manually redeem the funds',
        ]}
      />
      <SwapMoneroRecoveryButton swap={swap} size="small" variant="contained" />
    </Box>
  );
};

/**
 * Sub-component for displaying alerts when the swap is in a state with no timelock info.
 * @param swap - The swap information.
 * @param punishTimelockOffset - The punish timelock offset.
 * @returns JSX.Element
 */
const BitcoinLockedNoTimelockExpiredStateAlert = ({
  timelock,
  punishTimelockOffset,
}: {
  timelock: SwapTimelockInfoNone;
  punishTimelockOffset: number;
}) => (
  <MessageList
    messages={[
      <>
        Your Bitcoin is locked. If the swap is not completed in approximately{' '}
        <HumanizedBitcoinBlockDuration blocks={timelock.None.blocks_left} />,
        you need to refund
      </>,
      <>
        You will lose your funds if you do not refund or complete the swap
        within{' '}
        <HumanizedBitcoinBlockDuration
          blocks={timelock.None.blocks_left + punishTimelockOffset}
        />
      </>,
    ]}
  />
);

/**
 * Sub-component for displaying alerts when the swap timelock is expired
 * The swap could be cancelled but not necessarily (the transaction might not have been published yet)
 * But it doesn't matter because the swap cannot be completed anymore
 * @param swap - The swap information.
 * @returns JSX.Element
 */
const BitcoinPossiblyCancelledAlert = ({
  swap,
  timelock,
}: {
  swap: GetSwapInfoResponse;
  timelock: SwapTimelockInfoCancelled;
}) => {
  const classes = useStyles();
  return (
    <Box className={classes.box}>
      <MessageList
        messages={[
          'The swap was cancelled because it did not complete in time',
          'You must resume the swap immediately to refund your Bitcoin. If that fails, you can manually refund it',
          <>
            You will lose your funds if you do not refund within{' '}
            <HumanizedBitcoinBlockDuration
              blocks={timelock.Cancel.blocks_left}
            />
          </>,
        ]}
      />
      <SwapCancelRefundButton swap={swap} size="small" variant="contained" />
    </Box>
  );
};

/**
 * Sub-component for displaying alerts requiring immediate action.
 * @returns JSX.Element
 */
const ImmediateActionAlert = () => (
  <>Resume the swap immediately to avoid losing your funds</>
);

/**
 * Main component for displaying the appropriate swap alert status text.
 * @param swap - The swap information.
 * @returns JSX.Element | null
 */
function SwapAlertStatusText({
  swap,
}: {
  swap: GetSwapInfoResponseRunningSwap;
}) {
  switch (swap.stateName) {
    // This is the state where the swap is safe because the other party has redeemed the Bitcoin
    // It cannot be punished anymore
    case SwapStateName.BtcRedeemed:
      return <BitcoinRedeemedStateAlert swap={swap} />;

    // These are states that are at risk of punishment because the Bitcoin have been locked
    // but has not been redeemed yet by the other party
    case SwapStateName.BtcLocked:
    case SwapStateName.XmrLockProofReceived:
    case SwapStateName.XmrLocked:
    case SwapStateName.EncSigSent:
    case SwapStateName.CancelTimelockExpired:
    case SwapStateName.BtcCancelled:
      if (swap.timelock !== null) {
        if (isSwapTimelockInfoNone(swap.timelock)) {
          return (
            <BitcoinLockedNoTimelockExpiredStateAlert
              punishTimelockOffset={swap.punishTimelock}
              timelock={swap.timelock}
            />
          );
        }

        if (isSwapTimelockInfoCancelled(swap.timelock)) {
          return (
            <BitcoinPossiblyCancelledAlert
              timelock={swap.timelock}
              swap={swap}
            />
          );
        }

        if (isSwapTimelockInfoPunished(swap.timelock)) {
          return <ImmediateActionAlert />;
        }

        // We have covered all possible timelock states above
        // If we reach this point, it means we have missed a case
        return exhaustiveGuard(swap.timelock);
      }
      return <ImmediateActionAlert />;
    default:
      return exhaustiveGuard(swap.stateName);
  }
}

/**
 * Main component for displaying the swap status alert.
 * @param swap - The swap information.
 * @returns JSX.Element | null
 */
export default function SwapStatusAlert({
  swap,
}: {
  swap: GetSwapInfoResponse;
}): JSX.Element | null {
  // If the swap is not running, there is no need to display the alert
  // This is either because the swap is finished or has not started yet (e.g. in the setup phase, no Bitcoin locked)
  if (!isGetSwapInfoResponseRunningSwap(swap)) {
    return null;
  }

  return (
    <Alert
      key={swap.swapId}
      severity="warning"
      action={<SwapResumeButton swap={swap} />}
      variant="filled"
    >
      <AlertTitle>
        Swap {swap.swapId.substring(0, 5)}... is unfinished
      </AlertTitle>
      <SwapAlertStatusText swap={swap} />
    </Alert>
  );
}
