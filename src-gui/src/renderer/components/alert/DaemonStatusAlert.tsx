import { Button, CircularProgress } from "@material-ui/core";
import { Alert, AlertProps } from "@material-ui/lab";
import { TauriContextInitializationProgress } from "models/tauriModel";
import { useNavigate } from "react-router-dom";
import { useAppSelector } from "store/hooks";
import { exhaustiveGuard } from "utils/typescriptUtils";

function LoadingSpinnerAlert({ ...rest }: AlertProps) {
  return <Alert icon={<CircularProgress size={22} />} {...rest} />;
}

export default function DaemonStatusAlert() {
  const contextStatus = useAppSelector((s) => s.rpc.status);
  const navigate = useNavigate();

  if (contextStatus === null) {
    return <Alert severity="info">The daemon is not running</Alert>;
  }

  switch (contextStatus.type) {
    case "Initializing":
      switch (contextStatus.content) {
        case TauriContextInitializationProgress.OpeningBitcoinWallet:
          return (
            <LoadingSpinnerAlert severity="warning">
              Connecting to the Bitcoin network
            </LoadingSpinnerAlert>
          );
        case TauriContextInitializationProgress.OpeningMoneroWallet:
          return (
            <LoadingSpinnerAlert severity="warning">
              Connecting to the Monero network
            </LoadingSpinnerAlert>
          );
        case TauriContextInitializationProgress.OpeningDatabase:
          return (
            <LoadingSpinnerAlert severity="warning">
              Opening the local database
            </LoadingSpinnerAlert>
          );
      }
      break;
    case "Available":
      return <Alert severity="success">The daemon is running</Alert>;
    case "Failed":
      return (
        <Alert
          severity="error"
          action={
            <Button
              size="small"
              variant="outlined"
              onClick={() => navigate("/help")}
            >
              View Logs
            </Button>
          }
        >
          The daemon has stopped unexpectedly
        </Alert>
      );
    default:
      return exhaustiveGuard(contextStatus);
  }
}
