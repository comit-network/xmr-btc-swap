import { Box, DialogContentText } from "@material-ui/core";
import { SwapSpawnType } from "models/cliModel";
import { SwapStateProcessExited } from "models/storeModel";
import { useActiveSwapInfo, useAppSelector } from "store/hooks";
import CliLogsBox from "../../../../other/RenderedCliLog";

export default function ProcessExitedAndNotDonePage({
  state,
}: {
  state: SwapStateProcessExited;
}) {
  const swap = useActiveSwapInfo();
  const logs = useAppSelector((s) => s.swap.logs);
  const spawnType = useAppSelector((s) => s.swap.spawnType);

  function getText() {
    const isCancelRefund = spawnType === SwapSpawnType.CANCEL_REFUND;
    const hasRpcError = state.rpcError != null;
    const hasSwap = swap != null;

    const messages = [];

    messages.push(
      isCancelRefund
        ? "The manual cancel and refund was unsuccessful."
        : "The swap exited unexpectedly without completing.",
    );

    if (!hasSwap && !isCancelRefund) {
      messages.push("No funds were locked.");
    }

    messages.push(
      hasRpcError
        ? "Check the error and the logs below for more information."
        : "Check the logs below for more information.",
    );

    if (hasSwap) {
      messages.push(`The swap is in the "${swap.state_name}" state.`);
      if (!isCancelRefund) {
        messages.push(
          "Try resuming the swap or attempt to initiate a manual cancel and refund.",
        );
      }
    }

    return messages.join(" ");
  }

  return (
    <Box>
      <DialogContentText>{getText()}</DialogContentText>
      <Box
        style={{
          display: "flex",
          flexDirection: "column",
          gap: "0.5rem",
        }}
      >
        {state.rpcError && (
          <CliLogsBox
            logs={[state.rpcError]}
            label="Error returned by the Swap Daemon"
          />
        )}
        <CliLogsBox logs={logs} label="Logs relevant to the swap" />
      </Box>
    </Box>
  );
}
