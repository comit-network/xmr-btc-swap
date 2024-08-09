import {
  Button,
  ButtonProps,
  CircularProgress,
  IconButton,
  Tooltip,
} from "@material-ui/core";
import { ReactElement, ReactNode, useEffect, useState } from "react";
import { useSnackbar } from "notistack";
import { useAppSelector } from "store/hooks";
import { RpcProcessStateType } from "models/rpcModel";
import { isExternalRpc } from "store/config";

function IpcButtonTooltip({
  requiresRpcAndNotReady,
  children,
  processType,
  tooltipTitle,
}: {
  requiresRpcAndNotReady: boolean;
  children: ReactElement;
  processType: RpcProcessStateType;
  tooltipTitle?: string;
}) {
  if (tooltipTitle) {
    return <Tooltip title={tooltipTitle}>{children}</Tooltip>;
  }

  const getMessage = () => {
    if (!requiresRpcAndNotReady) return "";

    switch (processType) {
      case RpcProcessStateType.LISTENING_FOR_CONNECTIONS:
        return "";
      case RpcProcessStateType.STARTED:
        return "Cannot execute this action because the Swap Daemon is still starting and not yet ready to accept connections. Please wait a moment and try again";
      case RpcProcessStateType.EXITED:
        return "Cannot execute this action because the Swap Daemon has been stopped. Please start the Swap Daemon again to continue";
      case RpcProcessStateType.NOT_STARTED:
        return "Cannot execute this action because the Swap Daemon has not been started yet. Please start the Swap Daemon first";
      default:
        return "";
    }
  };

  return (
    <Tooltip title={getMessage()} color="red">
      {children}
    </Tooltip>
  );
}

interface IpcInvokeButtonProps<T> {
  ipcArgs: unknown[];
  ipcChannel: string;
  onSuccess?: (data: T) => void;
  isLoadingOverride?: boolean;
  isIconButton?: boolean;
  loadIcon?: ReactNode;
  requiresRpc?: boolean;
  disabled?: boolean;
  displayErrorSnackbar?: boolean;
  tooltipTitle?: string;
}

const DELAY_BEFORE_SHOWING_LOADING_MS = 0;

export default function IpcInvokeButton<T>({
  disabled,
  ipcChannel,
  ipcArgs,
  onSuccess,
  onClick,
  endIcon,
  loadIcon,
  isLoadingOverride,
  isIconButton,
  requiresRpc,
  displayErrorSnackbar,
  tooltipTitle,
  ...rest
}: IpcInvokeButtonProps<T> & ButtonProps) {
  const { enqueueSnackbar } = useSnackbar();

  const rpcProcessType = useAppSelector((state) => state.rpc.process.type);
  const isRpcReady =
    rpcProcessType === RpcProcessStateType.LISTENING_FOR_CONNECTIONS;
  const [isPending, setIsPending] = useState(false);
  const [hasMinLoadingTimePassed, setHasMinLoadingTimePassed] = useState(false);

  const isLoading = (isPending && hasMinLoadingTimePassed) || isLoadingOverride;
  const actualEndIcon = isLoading
    ? loadIcon || <CircularProgress size="1rem" />
    : endIcon;

  useEffect(() => {
    setHasMinLoadingTimePassed(false);
    setTimeout(
      () => setHasMinLoadingTimePassed(true),
      DELAY_BEFORE_SHOWING_LOADING_MS,
    );
  }, [isPending]);

  async function handleClick(event: React.MouseEvent<HTMLButtonElement>) {
    onClick?.(event);

    if (!isPending) {
      setIsPending(true);
      try {
        // const result = await ipcRenderer.invoke(ipcChannel, ...ipcArgs);
        throw new Error("Not implemented");
        // onSuccess?.(result);
      } catch (e: unknown) {
        if (displayErrorSnackbar) {
          enqueueSnackbar((e as Error).message, {
            autoHideDuration: 60 * 1000,
            variant: "error",
          });
        }
      } finally {
        setIsPending(false);
      }
    }
  }

  const requiresRpcAndNotReady =
    !!requiresRpc && !isRpcReady && !isExternalRpc();
  const isDisabled = disabled || requiresRpcAndNotReady || isLoading;

  return (
    <IpcButtonTooltip
      requiresRpcAndNotReady={requiresRpcAndNotReady}
      processType={rpcProcessType}
      tooltipTitle={tooltipTitle}
    >
      <span>
        {isIconButton ? (
          <IconButton
            onClick={handleClick}
            disabled={isDisabled}
            {...(rest as any)}
          >
            {actualEndIcon}
          </IconButton>
        ) : (
          <Button
            onClick={handleClick}
            disabled={isDisabled}
            endIcon={actualEndIcon}
            {...rest}
          />
        )}
      </span>
    </IpcButtonTooltip>
  );
}

IpcInvokeButton.defaultProps = {
  requiresRpc: true,
  disabled: false,
  onSuccess: undefined,
  isLoadingOverride: false,
  isIconButton: false,
  loadIcon: undefined,
  displayErrorSnackbar: true,
};
