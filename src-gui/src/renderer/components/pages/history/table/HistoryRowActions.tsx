import { Tooltip } from "@material-ui/core";
import { ButtonProps } from "@material-ui/core/Button/Button";
import { green, red } from "@material-ui/core/colors";
import DoneIcon from "@material-ui/icons/Done";
import ErrorIcon from "@material-ui/icons/Error";
import PlayArrowIcon from "@material-ui/icons/PlayArrow";
import { GetSwapInfoResponse } from "models/tauriModel";
import {
  BobStateName,
  GetSwapInfoResponseExt,
  isBobStateNamePossiblyCancellableSwap,
  isBobStateNamePossiblyRefundableSwap,
} from "models/tauriModelExt";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { resumeSwap } from "renderer/rpc";

export function SwapResumeButton({
  swap,
  ...props
}: ButtonProps & { swap: GetSwapInfoResponse }) {
  return (
    <PromiseInvokeButton
      variant="contained"
      color="primary"
      disabled={swap.completed}
      endIcon={<PlayArrowIcon />}
      onClick={() => resumeSwap(swap.swap_id)}
      {...props}
    >
      Resume
    </PromiseInvokeButton>
  );
}

export function SwapCancelRefundButton({
  swap,
  ...props
}: { swap: GetSwapInfoResponseExt } & ButtonProps) {
  const cancelOrRefundable =
    isBobStateNamePossiblyCancellableSwap(swap.state_name) ||
    isBobStateNamePossiblyRefundableSwap(swap.state_name);

  if (!cancelOrRefundable) {
    return <></>;
  }

  return (
    <PromiseInvokeButton
      displayErrorSnackbar={false}
      {...props}
      onClick={async () => {
        // TODO: Implement this using the Tauri RPC
        throw new Error("Not implemented");
      }}
    >
      Attempt manual Cancel & Refund
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

  // TODO: Display a button here to attempt a cooperative redeem
  // See this PR: https://github.com/UnstoppableSwap/unstoppableswap-gui/pull/212
  if (swap.state_name === BobStateName.BtcPunished) {
    return (
      <Tooltip title="This swap is completed. You have been punished.">
        <ErrorIcon style={{ color: red[500] }} />
      </Tooltip>
    );
  }

  return <SwapResumeButton swap={swap} />;
}
