import { Box, DialogContentText } from "@material-ui/core";
import { SwapSpawnType } from "models/cliModel";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { useActiveSwapInfo, useAppSelector } from "store/hooks";
import CliLogsBox from "../../../../other/RenderedCliLog";

export default function ProcessExitedAndNotDonePage({
  currState,
}: {
  currState: TauriSwapProgressEventContent<"Released">;
}) {
  const swap = useActiveSwapInfo();
  const logs = useAppSelector((s) => s.swap.logs);
  const spawnType = useAppSelector((s) => s.swap.spawnType);

  function getText() {
    const isCancelRefund = spawnType === SwapSpawnType.CANCEL_REFUND;
    const hasRpcError = currState.error != null;
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
        {currState.error != null && (
          <CliLogsBox logs={[currState.error]} label="Error" />
        )}
        <CliLogsBox logs={logs} label="Logs relevant to the swap" />
      </Box>
    </Box>
  );
}
