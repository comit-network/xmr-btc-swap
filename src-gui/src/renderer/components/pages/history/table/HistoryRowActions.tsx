import { Tooltip } from '@material-ui/core';
import Button, { ButtonProps } from '@material-ui/core/Button/Button';
import DoneIcon from '@material-ui/icons/Done';
import ErrorIcon from '@material-ui/icons/Error';
import { green, red } from '@material-ui/core/colors';
import PlayArrowIcon from '@material-ui/icons/PlayArrow';
import IpcInvokeButton from '../../../IpcInvokeButton';
import {
  GetSwapInfoResponse,
  SwapStateName,
  isSwapStateNamePossiblyCancellableSwap,
  isSwapStateNamePossiblyRefundableSwap,
} from '../../../../../models/rpcModel';

export function SwapResumeButton({
  swap,
  ...props
}: { swap: GetSwapInfoResponse } & ButtonProps) {
  return (
    <IpcInvokeButton
      variant="contained"
      color="primary"
      disabled={swap.completed}
      ipcChannel="spawn-resume-swap"
      ipcArgs={[swap.swapId]}
      endIcon={<PlayArrowIcon />}
      requiresRpc
      {...props}
    >
      Resume
    </IpcInvokeButton>
  );
}

export function SwapCancelRefundButton({
  swap,
  ...props
}: { swap: GetSwapInfoResponse } & ButtonProps) {
  const cancelOrRefundable =
    isSwapStateNamePossiblyCancellableSwap(swap.stateName) ||
    isSwapStateNamePossiblyRefundableSwap(swap.stateName);

  if (!cancelOrRefundable) {
    return <></>;
  }

  return (
    <IpcInvokeButton
      ipcChannel="spawn-cancel-refund"
      ipcArgs={[swap.swapId]}
      requiresRpc
      displayErrorSnackbar={false}
      {...props}
    >
      Attempt manual Cancel & Refund
    </IpcInvokeButton>
  );
}

export default function HistoryRowActions({
  swap,
}: {
  swap: GetSwapInfoResponse;
}) {
  if (swap.stateName === SwapStateName.XmrRedeemed) {
    return (
      <Tooltip title="The swap is completed because you have redeemed the XMR">
        <DoneIcon style={{ color: green[500] }} />
      </Tooltip>
    );
  }

  if (swap.stateName === SwapStateName.BtcRefunded) {
    return (
      <Tooltip title="The swap is completed because your BTC have been refunded">
        <DoneIcon style={{ color: green[500] }} />
      </Tooltip>
    );
  }

  if (swap.stateName === SwapStateName.BtcPunished) {
    return (
      <Tooltip title="The swap is completed because you have been punished">
        <ErrorIcon style={{ color: red[500] }} />
      </Tooltip>
    );
  }

  return <SwapResumeButton swap={swap} />;
}
