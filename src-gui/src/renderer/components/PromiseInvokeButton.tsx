import { Button, ButtonProps, IconButton, Tooltip } from "@material-ui/core";
import CircularProgress from "@material-ui/core/CircularProgress";
import { useSnackbar } from "notistack";
import { ReactNode, useEffect, useState } from "react";

interface IpcInvokeButtonProps<T> {
  onSuccess?: (data: T) => void;
  onClick: () => Promise<T>;
  isLoadingOverride?: boolean;
  isIconButton?: boolean;
  loadIcon?: ReactNode;
  disabled?: boolean;
  displayErrorSnackbar?: boolean;
  tooltipTitle?: string;
}

export default function PromiseInvokeButton<T>({
  disabled,
  onSuccess,
  onClick,
  endIcon,
  loadIcon,
  isLoadingOverride,
  isIconButton,
  displayErrorSnackbar,
  tooltipTitle,
  ...rest
}: IpcInvokeButtonProps<T> & ButtonProps) {
  const { enqueueSnackbar } = useSnackbar();

  const [isPending, setIsPending] = useState(false);
  const [hasMinLoadingTimePassed, setHasMinLoadingTimePassed] = useState(false);

  const isLoading = (isPending && hasMinLoadingTimePassed) || isLoadingOverride;
  const actualEndIcon = isLoading
    ? loadIcon || <CircularProgress size="1em" />
    : endIcon;

  useEffect(() => {
    setHasMinLoadingTimePassed(false);
    setTimeout(() => setHasMinLoadingTimePassed(true), 100);
  }, [isPending]);

  async function handleClick(event: React.MouseEvent<HTMLButtonElement>) {
    if (!isPending) {
      try {
        setIsPending(true);
        let result = await onClick();
        onSuccess?.(result);
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

  const isDisabled = disabled || isLoading;

  return (
    <Tooltip title={tooltipTitle}>
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
    </Tooltip>
  );
}
