import { Tooltip } from "@mui/material";
import { ButtonProps } from "@mui/material/Button";
import { green, red } from "@mui/material/colors";
import DoneIcon from "@mui/icons-material/Done";
import ErrorIcon from "@mui/icons-material/Error";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import { GetSwapInfoResponse } from "models/tauriModel";
import {
  BobStateName,
  GetSwapInfoResponseExt,
  isBobStateNamePossiblyCancellableSwap,
  isBobStateNamePossiblyRefundableSwap,
} from "models/tauriModelExt";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { resumeSwap, suspendCurrentSwap } from "renderer/rpc";
import {
  useIsSpecificSwapRunning,
  useIsSwapRunning,
  useIsSwapRunningAndHasFundsLocked,
} from "store/hooks";
import { useNavigate } from "react-router-dom";

export function SwapResumeButton({
  swap,
  children,
  ...props
}: ButtonProps & { swap: GetSwapInfoResponse }) {
  const navigate = useNavigate();

  // We cannot resume at all if the swap of this button is already running
  const isAlreadyRunning = useIsSpecificSwapRunning(swap.swap_id);

  // If another swap is running, we can resume but only if no funds have been locked
  // for that swap. If funds have been locked, we cannot resume. If no funds have been locked,
  // we suspend the other swap and resume this one.
  const isAnotherSwapRunningAndHasFundsLocked =
    useIsSwapRunningAndHasFundsLocked() && !isAlreadyRunning;

  async function resume() {
    // We always suspend the current swap first
    // If that swap has any funds locked, the button will be disabled
    // and this function will not be called
    // If no swap is running, this is a no-op
    await suspendCurrentSwap();

    // Now resume this swap
    await resumeSwap(swap.swap_id);

    // Navigate to the swap page
    navigate(`/swap`);
  }

  const tooltipTitle = isAlreadyRunning
    ? "This swap is already running"
    : isAnotherSwapRunningAndHasFundsLocked
      ? "Another swap is running. Suspend it first before resuming this one"
      : undefined;

  return (
    <PromiseInvokeButton
      variant="contained"
      color="primary"
      disabled={
        swap.completed ||
        isAlreadyRunning ||
        isAnotherSwapRunningAndHasFundsLocked
      }
      tooltipTitle={tooltipTitle}
      endIcon={<PlayArrowIcon />}
      onInvoke={resume}
      {...props}
    >
      {children}
    </PromiseInvokeButton>
  );
}

export default function HistoryRowActions(swap: GetSwapInfoResponse) {
  if (swap.state_name === BobStateName.XmrRedeemed) {
    return (
      <Tooltip title="This swap is completed. You have redeemed the Monero.">
        <DoneIcon style={{ color: green[500] }} />
      </Tooltip>
    );
  }

  if (swap.state_name === BobStateName.BtcRefunded) {
    return (
      <Tooltip title="This swap is completed. Your Bitcoin has been refunded.">
        <DoneIcon style={{ color: green[500] }} />
      </Tooltip>
    );
  }

  if (swap.state_name === BobStateName.BtcEarlyRefunded) {
    return (
      <Tooltip title="This swap is completed. Your Bitcoin has been refunded.">
        <DoneIcon style={{ color: green[500] }} />
      </Tooltip>
    );
  }

  if (swap.state_name === BobStateName.BtcPunished) {
    return (
      <Tooltip title="You have been punished. You can attempt to recover the Monero with the help of the other party but that is not guaranteed to work">
        <SwapResumeButton swap={swap} size="small">
          Attempt recovery
        </SwapResumeButton>
      </Tooltip>
    );
  }

  return <SwapResumeButton swap={swap}>Resume</SwapResumeButton>;
}
