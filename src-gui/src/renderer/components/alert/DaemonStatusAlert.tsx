import { Box, Button, LinearProgress, makeStyles, Badge } from "@material-ui/core";
import { Alert } from "@material-ui/lab";
import { useNavigate } from "react-router-dom";
import { useAppSelector, usePendingBackgroundProcesses } from "store/hooks";
import { exhaustiveGuard } from "utils/typescriptUtils";
import { LoadingSpinnerAlert } from "./LoadingSpinnerAlert";
import { bytesToMb } from "utils/conversionUtils";
import { TauriBackgroundProgress, TauriContextStatusEvent } from "models/tauriModel";
import { useEffect, useState } from "react";
import TruncatedText from "../other/TruncatedText";
import BitcoinIcon from "../icons/BitcoinIcon";
import MoneroIcon from "../icons/MoneroIcon";
import TorIcon from "../icons/TorIcon";

const useStyles = makeStyles((theme) => ({
  innerAlert: {
    display: "flex",
    flexDirection: "column",
    gap: theme.spacing(2),
  },
}));

function AlertWithLinearProgress({ title, progress, icon, count }: {
  title: React.ReactNode,
  progress: number | null,
  icon?: React.ReactNode | null,
  count?: number
}) {
  const BUFFER_PROGRESS_ADDITION_MAX = 20;

  const [bufferProgressAddition, setBufferProgressAddition] = useState(Math.random() * BUFFER_PROGRESS_ADDITION_MAX);

  useEffect(() => {
    setBufferProgressAddition(Math.random() * BUFFER_PROGRESS_ADDITION_MAX);
  }, [progress]);

  let displayIcon = icon ?? null;
  if (icon && count && count > 1) {
    displayIcon = (
      <Badge badgeContent={count} color="error">
        {icon}
      </Badge>
    );
  }

  // If the progress is already at 100%, but not finished yet we show an indeterminate progress bar
  // as it'd be confusing to show a 100% progress bar for longer than a second or so.
  return <Alert severity="info" icon={displayIcon}>
    <Box style={{ display: "flex", flexDirection: "column", gap: "0.5rem" }}>
      {title}
      {(progress === null || progress === 0 || progress >= 100) ? (
        <LinearProgress variant="indeterminate" />
      ) : (
        <LinearProgress variant="buffer" value={progress} valueBuffer={Math.min(progress + bufferProgressAddition, 100)} />
      )}
    </Box>
  </Alert>
}

function PartialInitStatus({ status, totalOfType, classes }: {
  status: TauriBackgroundProgress,
  totalOfType: number,
  classes: ReturnType<typeof useStyles>
}) {
  if (status.progress.type === "Completed") {
    return null;
  }

  switch (status.componentName) {
    case "EstablishingTorCircuits":
      return (
        <AlertWithLinearProgress
          title={
            <>
              Establishing Tor circuits
            </>
          }
          progress={status.progress.content.frac * 100}
          count={totalOfType}
          icon={<TorIcon />}
        />
      );
    case "SyncingBitcoinWallet":
      const progressValue =
        status.progress.content?.type === "Known" ?
        (status.progress.content?.content?.consumed / status.progress.content?.content?.total) * 100 : null;

      return (
        <AlertWithLinearProgress
          title={
            <>
              Syncing Bitcoin wallet
            </>
          }
          progress={progressValue}
          icon={<BitcoinIcon />}
          count={totalOfType}
        />
      );
    case "FullScanningBitcoinWallet":
      const fullScanProgressValue = status.progress.content?.type === "Known" ? (status.progress.content?.content?.current_index / status.progress.content?.content?.assumed_total) * 100 : null;
      return (
        <AlertWithLinearProgress
          title={
            <>
              Full scan of Bitcoin wallet (one time operation)
            </>
          }
          progress={fullScanProgressValue}
          icon={<BitcoinIcon />}
          count={totalOfType}
        />
      );
    case "OpeningBitcoinWallet":
      return (
        <LoadingSpinnerAlert severity="info">
          <>
            Opening Bitcoin wallet
          </>
        </LoadingSpinnerAlert>
      );
    case "DownloadingMoneroWalletRpc":
      const moneroRpcTitle = `Downloading and verifying the Monero wallet RPC (${bytesToMb(status.progress.content.size).toFixed(2)} MB)`;
      return (
        <AlertWithLinearProgress
          title={
            <>
              {moneroRpcTitle}
            </>
          }
          progress={status.progress.content.progress}
          icon={<MoneroIcon />}
          count={totalOfType}
        />
      );
    case "OpeningMoneroWallet":
      return (
        <LoadingSpinnerAlert severity="info">
          <>
            Opening the Monero wallet
          </>
        </LoadingSpinnerAlert>
      );
    case "OpeningDatabase":
      return (
        <LoadingSpinnerAlert severity="info">
          <>
            Opening the local database
          </>
        </LoadingSpinnerAlert>
      );
    case "BackgroundRefund":
      return (
        <LoadingSpinnerAlert severity="info">
          <>
            Refunding swap <TruncatedText limit={10}>{status.progress.content.swap_id}</TruncatedText>
          </>
        </LoadingSpinnerAlert>
      );
    default:
      return exhaustiveGuard(status);
  }
}

export default function DaemonStatusAlert() {
  const contextStatus = useAppSelector((s) => s.rpc.status);
  const navigate = useNavigate();

  if (contextStatus === null || contextStatus === TauriContextStatusEvent.NotInitialized) {
    return <LoadingSpinnerAlert severity="warning">Checking for available remote nodes</LoadingSpinnerAlert>;
  }

  switch (contextStatus) {
    case TauriContextStatusEvent.Initializing:
      return <LoadingSpinnerAlert severity="warning">Core components are loading</LoadingSpinnerAlert>;
    case TauriContextStatusEvent.Available:
      return <Alert severity="success">The daemon is running</Alert>;
    case TauriContextStatusEvent.Failed:
      return (
        <Alert
          severity="error"
          action={
            <Button
              size="small"
              variant="outlined"
              onClick={() => navigate("/settings#daemon-control-box")}
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

export function BackgroundProgressAlerts() {
  const backgroundProgress = usePendingBackgroundProcesses();
  const classes = useStyles();

  if (backgroundProgress.length === 0) {
    return null;
  }

  const componentCounts: Record<string, number> = {};
  backgroundProgress.forEach(([, status]) => {
    componentCounts[status.componentName] = (componentCounts[status.componentName] || 0) + 1;
  });

  const renderedComponentNames = new Set<string>();
  const uniqueBackgroundProcesses = backgroundProgress.filter(([, status]) => {
    if (!renderedComponentNames.has(status.componentName)) {
      renderedComponentNames.add(status.componentName);
      return true;
    }
    return false;
  });

  return uniqueBackgroundProcesses.map(([id, status]) => (
    <PartialInitStatus
      key={id}
      status={status}
      classes={classes}
      totalOfType={componentCounts[status.componentName]}
    />
  ));
}