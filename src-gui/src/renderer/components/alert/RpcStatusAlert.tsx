import { CircularProgress } from "@material-ui/core";
import { Alert } from "@material-ui/lab";
import { RpcProcessStateType } from "models/rpcModel";
import { useAppSelector } from "store/hooks";

// TODO: Reimplement this using Tauri
// Currently the RPC process is always available, so this component is not needed
// since the UI is only displayed when the RPC process is available
export default function RpcStatusAlert() {
  const rpcProcess = useAppSelector((s) => s.rpc.process);
  if (rpcProcess.type === RpcProcessStateType.STARTED) {
    return (
      <Alert severity="warning" icon={<CircularProgress size={22} />}>
        The swap daemon is starting
      </Alert>
    );
  }
  if (rpcProcess.type === RpcProcessStateType.LISTENING_FOR_CONNECTIONS) {
    return <Alert severity="success">The swap daemon is running</Alert>;
  }
  if (rpcProcess.type === RpcProcessStateType.NOT_STARTED) {
    return <Alert severity="warning">The swap daemon is being started</Alert>;
  }
  if (rpcProcess.type === RpcProcessStateType.EXITED) {
    return (
      <Alert severity="error">The swap daemon has stopped unexpectedly</Alert>
    );
  }
  return <></>;
}
