import { Box, Button, LinearProgress, makeStyles } from "@material-ui/core";
import { Alert } from "@material-ui/lab";
import { useNavigate } from "react-router-dom";
import { useAppSelector } from "store/hooks";
import { exhaustiveGuard } from "utils/typescriptUtils";
import { LoadingSpinnerAlert } from "./LoadingSpinnerAlert";
import { bytesToMb } from "utils/conversionUtils";
import { TauriPartialInitProgress } from "models/tauriModel";

const useStyles = makeStyles((theme) => ({
  innerAlert: {
    display: "flex",
    flexDirection: "column",
    gap: theme.spacing(2),
  },
}));

function PartialInitStatus({ status, classes }: {
  status: TauriPartialInitProgress,
  classes: ReturnType<typeof useStyles>
}) {
  if (status.progress.type === "Completed") {
    return null;
  }

  switch (status.componentName) {
    case "OpeningBitcoinWallet":
      return (
        <LoadingSpinnerAlert severity="warning">
          Syncing internal Bitcoin wallet
        </LoadingSpinnerAlert>
      );
    case "DownloadingMoneroWalletRpc":
      return (
        <LoadingSpinnerAlert severity="warning">
          <Box className={classes.innerAlert}>
            <Box>
              Downloading and verifying the Monero wallet RPC (
              {bytesToMb(status.progress.content.size).toFixed(2)} MB)
            </Box>
            <LinearProgress variant="determinate" value={status.progress.content.progress} />
          </Box>
        </LoadingSpinnerAlert>
      );
    case "OpeningMoneroWallet":
      return (
        <LoadingSpinnerAlert severity="warning">
          Opening the Monero wallet
        </LoadingSpinnerAlert>
      );
    case "OpeningDatabase":
      return (
        <LoadingSpinnerAlert severity="warning">
          Opening the local database
        </LoadingSpinnerAlert>
      );
    case "EstablishingTorCircuits":
      return (
        <LoadingSpinnerAlert severity="warning">
          Establishing Tor circuits
        </LoadingSpinnerAlert>
      )
    default:
      return null;
  }
}

export default function DaemonStatusAlert() {
  const classes = useStyles();
  const contextStatus = useAppSelector((s) => s.rpc.status);
  const navigate = useNavigate();

  if (contextStatus === null || contextStatus.type === "NotInitialized") {
    return <LoadingSpinnerAlert severity="warning">Checking for available remote nodes</LoadingSpinnerAlert>;
  }

  switch (contextStatus.type) {
    case "Initializing":
      return contextStatus.content
        .map((status) => (
          <PartialInitStatus
            key={status.componentName}
            status={status}
            classes={classes}
          />
        ))
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
              onClick={() => navigate("/help#daemon-control-box")}
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
