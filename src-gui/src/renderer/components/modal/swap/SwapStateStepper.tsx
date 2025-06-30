import { Step, StepLabel, Stepper, Typography } from "@mui/material";
import { SwapState } from "models/storeModel";
import { useAppSelector } from "store/hooks";
import logger from "utils/logger";

export enum PathType {
  HAPPY_PATH = "happy path",
  UNHAPPY_PATH = "unhappy path",
}

type PathStep = [type: PathType, step: number, isError: boolean];

/**
 * Determines the current step in the swap process based on the previous and latest state.
 * @param prevState - The previous state of the swap process (null if it's the initial state)
 * @param latestState - The latest state of the swap process
 * @returns A tuple containing [PathType, activeStep, errorFlag]
 */
function getActiveStep(state: SwapState | null): PathStep | null {
  // In case we cannot infer a correct step from the state
  function fallbackStep(reason: string) {
    logger.error(
      `Unable to choose correct stepper type (reason: ${reason}, state: ${JSON.stringify(state)}`,
    );
    return null;
  }

  if (state === null) {
    return [PathType.HAPPY_PATH, 0, false];
  }

  const prevState = state.prev;
  const isReleased = state.curr.type === "Released";

  // If the swap is released we use the previous state to display the correct step
  const latestState = isReleased ? prevState : state.curr;

  // If the swap is released but we do not have a previous state we fallback
  if (latestState === null) {
    return fallbackStep(
      "Swap has been released but we do not have a previous state saved to display",
    );
  }

  // This should really never happen. For this statement to be true, the host has to submit a "Released" event twice
  if (latestState.type === "Released") {
    return fallbackStep(
      "Both the current and previous states are both of type 'Released'.",
    );
  }

  switch (latestState.type) {
    // Step 0: Initializing the swap
    // These states represent the very beginning of the swap process
    // No funds have been locked
    case "RequestingQuote":
    case "ReceivedQuote":
    case "WaitingForBtcDeposit":
    case "SwapSetupInflight":
      return [PathType.HAPPY_PATH, 0, isReleased];

    // Step 1: Waiting for Bitcoin lock confirmation
    // Bitcoin has been locked, waiting for the counterparty to lock their XMR
    case "BtcLockTxInMempool":
      // We only display the first step as completed if the Bitcoin lock has been confirmed
      if (
        latestState.content.btc_lock_confirmations !== undefined &&
        latestState.content.btc_lock_confirmations > 0
      ) {
        return [PathType.HAPPY_PATH, 1, isReleased];
      }
      return [PathType.HAPPY_PATH, 0, isReleased];

    // Still Step 1: Both Bitcoin and XMR have been locked, waiting for Monero lock to be confirmed
    case "XmrLockTxInMempool":
      return [PathType.HAPPY_PATH, 1, isReleased];

    // Step 2: Waiting for encrypted signature to be sent to Alice
    // and for Alice to redeem the Bitcoin
    case "XmrLocked":
    case "EncryptedSignatureSent":
      return [PathType.HAPPY_PATH, 2, isReleased];

    // Step 3: Waiting for XMR redemption
    // Bitcoin has been redeemed by Alice, now waiting for us to redeem Monero
    case "WaitingForXmrConfirmationsBeforeRedeem":
    case "RedeemingMonero":
      return [PathType.HAPPY_PATH, 3, isReleased];

    // Step 4: Swap completed successfully
    // XMR redemption transaction is in mempool, swap is essentially complete
    case "XmrRedeemInMempool":
      return [PathType.HAPPY_PATH, 4, false];

    // Unhappy Path States

    // Step 1: Cancel timelock has expired. Waiting for cancel transaction to be published
    case "CancelTimelockExpired":
      return [PathType.UNHAPPY_PATH, 0, isReleased];

    // Step 2: Swap has been cancelled. Waiting for Bitcoin to be refunded
    case "BtcCancelled":
      return [PathType.UNHAPPY_PATH, 1, isReleased];

    // Step 2: One of the two Bitcoin refund transactions have been published
    // but they haven't been confirmed yet
    case "BtcRefundPublished":
    case "BtcEarlyRefundPublished":
      return [PathType.UNHAPPY_PATH, 1, isReleased];

    // Step 2: One of the two Bitcoin refund transactions have been confirmed
    case "BtcRefunded":
    case "BtcEarlyRefunded":
      return [PathType.UNHAPPY_PATH, 2, false];

    // Step 2 (Failed): Failed to refund Bitcoin
    // The timelock expired before we could refund, resulting in punishment
    case "BtcPunished":
      return [PathType.UNHAPPY_PATH, 1, true];

    // Attempting cooperative redemption after punishment
    case "AttemptingCooperativeRedeem":
    case "CooperativeRedeemAccepted":
      return [PathType.UNHAPPY_PATH, 1, isReleased];
    case "CooperativeRedeemRejected":
      return [PathType.UNHAPPY_PATH, 1, true];

    case "Resuming":
      return null;
    default:
      return fallbackStep("No step is assigned to the current state");
    // TODO: Make this guard work. It should force the compiler to check if we have covered all possible cases.
    // return exhaustiveGuard(latestState.type);
  }
}

function SwapStepper({
  steps,
  activeStep,
  error,
}: {
  steps: Array<{ label: string; duration: string }>;
  activeStep: number;
  error: boolean;
}) {
  return (
    <Stepper activeStep={activeStep}>
      {steps.map((step, index) => (
        <Step key={index}>
          <StepLabel
            optional={
              <Typography variant="caption">{step.duration}</Typography>
            }
            error={error && activeStep === index}
          >
            {step.label}
          </StepLabel>
        </Step>
      ))}
    </Stepper>
  );
}

const HAPPY_PATH_STEP_LABELS = [
  { label: "Locking your BTC", duration: "~12min" },
  { label: "They lock their XMR", duration: "~10min" },
  { label: "They redeem the BTC", duration: "~2min" },
  { label: "Redeeming your XMR", duration: "~10min" },
];

const UNHAPPY_PATH_STEP_LABELS = [
  { label: "Cancelling swap", duration: "~1min" },
  { label: "Attempting recovery", duration: "~5min" },
];

export default function SwapStateStepper({
  state,
}: {
  state: SwapState | null;
}) {
  const result = getActiveStep(state);

  if (result === null) {
    return null;
  }

  const [pathType, activeStep, error] = result;

  const steps =
    pathType === PathType.HAPPY_PATH
      ? HAPPY_PATH_STEP_LABELS
      : UNHAPPY_PATH_STEP_LABELS;

  return <SwapStepper steps={steps} activeStep={activeStep} error={error} />;
}
