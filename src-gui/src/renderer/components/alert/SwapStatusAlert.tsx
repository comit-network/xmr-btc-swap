import { Box, makeStyles } from "@material-ui/core";
import { Alert, AlertTitle } from "@material-ui/lab/";
import { GetSwapInfoResponse } from "models/tauriModel";
import {
  BobStateName,
  GetSwapInfoResponseExt,
  TimelockCancel,
  TimelockNone,
} from "models/tauriModelExt";
import { ReactNode } from "react";
import { exhaustiveGuard } from "utils/typescriptUtils";
import HumanizedBitcoinBlockDuration from "../other/HumanizedBitcoinBlockDuration";
import {
  SwapCancelRefundButton,
  SwapResumeButton,
} from "../pages/history/table/HistoryRowActions";
import { SwapMoneroRecoveryButton } from "../pages/history/table/SwapMoneroRecoveryButton";

const useStyles = makeStyles({
  box: {
    display: "flex",
    flexDirection: "column",
    gap: "0.5rem",
  },
  list: {
    padding: "0px",
    margin: "0px",
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
          "The Bitcoin has been redeemed by the other party",
          "There is no risk of losing funds. You can take your time",
          "The Monero will be automatically redeemed to the address you provided as soon as you resume the swap",
          "If this step fails, you can manually redeem the funds",
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
  timelock: TimelockNone;
  punishTimelockOffset: number;
}) => (
  <MessageList
    messages={[
      <>
        Your Bitcoin is locked. If the swap is not completed in approximately{" "}
        <HumanizedBitcoinBlockDuration blocks={timelock.content.blocks_left} />,
        you need to refund
      </>,
      <>
        You might lose your funds if you do not refund or complete the swap
        within{" "}
        <HumanizedBitcoinBlockDuration
          blocks={timelock.content.blocks_left + punishTimelockOffset}
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
  swap: GetSwapInfoResponseExt;
  timelock: TimelockCancel;
}) => {
  const classes = useStyles();
  return (
    <Box className={classes.box}>
      <MessageList
        messages={[
          "The swap was cancelled because it did not complete in time",
          "You must resume the swap immediately to refund your Bitcoin. If that fails, you can manually refund it",
          <>
            You might lose your funds if you do not refund within{" "}
            <HumanizedBitcoinBlockDuration
              blocks={timelock.content.blocks_left}
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
function SwapAlertStatusText({ swap }: { swap: GetSwapInfoResponseExt }) {
  switch (swap.state_name) {
    // This is the state where the swap is safe because the other party has redeemed the Bitcoin
    // It cannot be punished anymore
    case BobStateName.BtcRedeemed:
      return <BitcoinRedeemedStateAlert swap={swap} />;

    // These are states that are at risk of punishment because the Bitcoin have been locked
    // but has not been redeemed yet by the other party
    case BobStateName.BtcLocked:
    case BobStateName.XmrLockProofReceived:
    case BobStateName.XmrLocked:
    case BobStateName.EncSigSent:
    case BobStateName.CancelTimelockExpired:
    case BobStateName.BtcCancelled:
      if (swap.timelock != null) {
        switch (swap.timelock.type) {
          case "None":
            return (
              <BitcoinLockedNoTimelockExpiredStateAlert
                punishTimelockOffset={swap.punish_timelock}
                timelock={swap.timelock}
              />
            );

          case "Cancel":
            return (
              <BitcoinPossiblyCancelledAlert
                timelock={swap.timelock}
                swap={swap}
              />
            );
          case "Punish":
            return <ImmediateActionAlert />;

          default:
            // We have covered all possible timelock states above
            // If we reach this point, it means we have missed a case
            exhaustiveGuard(swap.timelock);
        }
      }
      return <ImmediateActionAlert />;
    default:
      // TODO: fix the exhaustive guard
      // return exhaustiveGuard(swap.state_name);
      return <></>;
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
  swap: GetSwapInfoResponseExt;
}): JSX.Element | null {
  // If the swap is completed, there is no need to display the alert
  // TODO: Here we should also check if the swap is in a state where any funds can be lost
  // TODO: If the no Bitcoin have been locked yet, we can safely ignore the swap
  if (swap.completed) {
    return null;
  }

  return (
    <Alert
      key={swap.swap_id}
      severity="warning"
      action={<SwapResumeButton swap={swap} />}
      variant="filled"
    >
      <AlertTitle>
        Swap {swap.swap_id.substring(0, 5)}... is unfinished
      </AlertTitle>
      <SwapAlertStatusText swap={swap} />
    </Alert>
  );
}
