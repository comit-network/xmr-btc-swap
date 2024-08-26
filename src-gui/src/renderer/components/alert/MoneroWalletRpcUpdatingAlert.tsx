import { Box, LinearProgress } from "@material-ui/core";
import { Alert } from "@material-ui/lab";
import { useAppSelector } from "store/hooks";

export default function MoneroWalletRpcUpdatingAlert() {
  // TODO: Reimplement this using Tauri Events
  return <></>;

  const updateState = useAppSelector(
    (s) => s.rpc.state.moneroWalletRpc.updateState,
  );

  if (updateState === false) {
    return null;
  }

  const progress = Number.parseFloat(
    updateState.progress.substring(0, updateState.progress.length - 1),
  );

  return (
    <Alert severity="info">
      <Box style={{ display: "flex", flexDirection: "column", gap: "0.5rem" }}>
        <span>The Monero wallet is updating. This may take a few moments</span>
        <LinearProgress
          variant="determinate"
          value={progress}
          title="Download progress"
        />
      </Box>
    </Alert>
  );
}
