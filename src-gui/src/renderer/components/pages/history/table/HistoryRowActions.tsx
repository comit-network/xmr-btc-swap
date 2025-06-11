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
import { resumeSwap } from "renderer/rpc";

export function SwapResumeButton({
  swap,
  children,
  ...props
}: ButtonProps & { swap: GetSwapInfoResponse }) {
  return (
    <PromiseInvokeButton
      variant="contained"
      color="primary"
      disabled={swap.completed}
      endIcon={<PlayArrowIcon />}
      onInvoke={() => resumeSwap(swap.swap_id)}
      {...props}
    >
      {children}
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
      onInvoke={async () => {
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
