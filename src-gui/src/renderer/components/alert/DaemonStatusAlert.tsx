import { Box, Button, LinearProgress, makeStyles } from "@material-ui/core";
import { Alert } from "@material-ui/lab";
import { TauriContextInitializationProgress } from "models/tauriModel";
import { useNavigate } from "react-router-dom";
import { useAppSelector } from "store/hooks";
import { exhaustiveGuard } from "utils/typescriptUtils";
import { LoadingSpinnerAlert } from "./LoadingSpinnerAlert";
import { bytesToMb } from "utils/conversionUtils";

const useStyles = makeStyles((theme) => ({
  innerAlert: {
    display: "flex",
    flexDirection: "column",
    gap: theme.spacing(2),
  },
}));

export default function DaemonStatusAlert() {
  const classes = useStyles();
  const contextStatus = useAppSelector((s) => s.rpc.status);
  const navigate = useNavigate();

  if (contextStatus === null) {
    return <LoadingSpinnerAlert severity="warning">Checking for available remote nodes</LoadingSpinnerAlert>;
  }

  switch (contextStatus.type) {
    case "Initializing":
      switch (contextStatus.content.type) {
        case "OpeningBitcoinWallet":
          return (
            <LoadingSpinnerAlert severity="warning">
              Connecting to the Bitcoin network
            </LoadingSpinnerAlert>
          );
        case "DownloadingMoneroWalletRpc":
          return (
            <LoadingSpinnerAlert severity="warning">
              <Box className={classes.innerAlert}>
                <Box>
                  Downloading and verifying the Monero wallet RPC (
                  {bytesToMb(contextStatus.content.content.size).toFixed(2)} MB)
                </Box>
                <LinearProgress variant="determinate" value={contextStatus.content.content.progress} />
              </Box>
            </LoadingSpinnerAlert >
          );
        case "OpeningMoneroWallet":
          return (
            <LoadingSpinnerAlert severity="warning">
              Connecting to the Monero network
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
              Connecting to the Tor network
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
