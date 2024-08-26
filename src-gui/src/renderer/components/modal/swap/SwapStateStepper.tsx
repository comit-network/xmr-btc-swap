import { Step, StepLabel, Stepper, Typography } from "@material-ui/core";
import { SwapSpawnType } from "models/cliModel";
import { BobStateName } from "models/tauriModelExt";
import {
  useActiveSwapInfo,
  useAppSelector,
  useIsSwapRunning,
} from "store/hooks";
import { exhaustiveGuard } from "utils/typescriptUtils";

export enum PathType {
  HAPPY_PATH = "happy path",
  UNHAPPY_PATH = "unhappy path",
}

// TODO: Consider using a TauriProgressEvent here instead of BobStateName
// TauriProgressEvent is always up to date, BobStateName is not (needs to be periodically fetched)
function getActiveStep(
  stateName: BobStateName | null,
  processExited: boolean,
): [PathType, number, boolean] {
  switch (stateName) {
    /// // Happy Path
    // Step: 0 (Waiting for Bitcoin lock tx to be published)
    case null:
      return [PathType.HAPPY_PATH, 0, false];
    case BobStateName.Started:
    case BobStateName.SwapSetupCompleted:
      return [PathType.HAPPY_PATH, 0, processExited];

    // Step: 1 (Waiting for Bitcoin Lock confirmation and XMR Lock Publication)
    // We have locked the Bitcoin and are waiting for the other party to lock their XMR
    case BobStateName.BtcLocked:
      return [PathType.HAPPY_PATH, 1, processExited];

    // Step: 2 (Waiting for XMR Lock confirmation)
    // We have locked the Bitcoin and the other party has locked their XMR
    case BobStateName.XmrLockProofReceived:
      return [PathType.HAPPY_PATH, 1, processExited];

    // Step: 3 (Sending Encrypted Signature and waiting for Bitcoin Redemption)
    // The XMR lock transaction has been confirmed
    // We now need to send the encrypted signature to the other party and wait for them to redeem the Bitcoin
    case BobStateName.XmrLocked:
    case BobStateName.EncSigSent:
      return [PathType.HAPPY_PATH, 2, processExited];

    // Step: 4 (Waiting for XMR Redemption)
    case BobStateName.BtcRedeemed:
      return [PathType.HAPPY_PATH, 3, processExited];

    // Step: 4 (Completed) (Swap completed, XMR redeemed)
    case BobStateName.XmrRedeemed:
      return [PathType.HAPPY_PATH, 4, false];

    // Edge Case of Happy Path where the swap is safely aborted. We "fail" at the first step.
    case BobStateName.SafelyAborted:
      return [PathType.HAPPY_PATH, 0, true];

    // // Unhappy Path
    // Step: 1 (Cancelling swap, checking if cancel transaction has been published already by the other party)
    case BobStateName.CancelTimelockExpired:
      return [PathType.UNHAPPY_PATH, 0, processExited];

    // Step: 2 (Attempt to publish the Bitcoin refund transaction)
    case BobStateName.BtcCancelled:
      return [PathType.UNHAPPY_PATH, 1, processExited];

    // Step: 2 (Completed) (Bitcoin refunded)
    case BobStateName.BtcRefunded:
      return [PathType.UNHAPPY_PATH, 2, false];

    // Step: 2 (We failed to publish the Bitcoin refund transaction)
    // We failed to publish the Bitcoin refund transaction because the timelock has expired.
    // We will be punished. Nothing we can do about it now.
    case BobStateName.BtcPunished:
      return [PathType.UNHAPPY_PATH, 1, true];
    default:
      return exhaustiveGuard(stateName);
  }
}

function HappyPathStepper({
  activeStep,
  error,
}: {
  activeStep: number;
  error: boolean;
}) {
  return (
    <Stepper activeStep={activeStep}>
      <Step key={0}>
        <StepLabel
          optional={<Typography variant="caption">~12min</Typography>}
          error={error && activeStep === 0}
        >
          Locking your BTC
        </StepLabel>
      </Step>
      <Step key={1}>
        <StepLabel
          optional={<Typography variant="caption">~18min</Typography>}
          error={error && activeStep === 1}
        >
          They lock their XMR
        </StepLabel>
      </Step>
      <Step key={2}>
        <StepLabel
          optional={<Typography variant="caption">~2min</Typography>}
          error={error && activeStep === 2}
        >
          They redeem the BTC
        </StepLabel>
      </Step>
      <Step key={3}>
        <StepLabel
          optional={<Typography variant="caption">~2min</Typography>}
          error={error && activeStep === 3}
        >
          Redeeming your XMR
        </StepLabel>
      </Step>
    </Stepper>
  );
}

function UnhappyPathStepper({
  activeStep,
  error,
}: {
  activeStep: number;
  error: boolean;
}) {
  return (
    <Stepper activeStep={activeStep}>
      <Step key={0}>
        <StepLabel
          optional={<Typography variant="caption">~20min</Typography>}
          error={error && activeStep === 0}
        >
          Cancelling swap
        </StepLabel>
      </Step>
      <Step key={1}>
        <StepLabel
          optional={<Typography variant="caption">~20min</Typography>}
          error={error && activeStep === 1}
        >
          Refunding your BTC
        </StepLabel>
      </Step>
    </Stepper>
  );
}

export default function SwapStateStepper() {
  // TODO: There's no equivalent of this with Tauri yet.
  const currentSwapSpawnType = useAppSelector((s) => s.swap.spawnType);

  const stateName = useActiveSwapInfo()?.state_name ?? null;
  const processExited = !useIsSwapRunning();
  const [pathType, activeStep, error] = getActiveStep(stateName, processExited);

  // TODO: Fix this to work with Tauri
  // If the current swap is being manually cancelled and refund, we want to show the unhappy path even though the current state is not a "unhappy" state
  if (currentSwapSpawnType === SwapSpawnType.CANCEL_REFUND) {
    return <UnhappyPathStepper activeStep={0} error={error} />;
  }

  if (pathType === PathType.HAPPY_PATH) {
    return <HappyPathStepper activeStep={activeStep} error={error} />;
  }
  return <UnhappyPathStepper activeStep={activeStep} error={error} />;
}
