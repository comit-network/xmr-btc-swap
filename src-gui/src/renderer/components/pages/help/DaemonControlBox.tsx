import { Box, makeStyles } from "@material-ui/core";
import FolderOpenIcon from "@material-ui/icons/FolderOpen";
import PlayArrowIcon from "@material-ui/icons/PlayArrow";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { useAppSelector } from "store/hooks";
import InfoBox from "../../modal/swap/InfoBox";
import CliLogsBox from "../../other/RenderedCliLog";
import { initializeContext } from "renderer/rpc";
import { relaunch } from "@tauri-apps/plugin-process";
import RotateLeftIcon from "@material-ui/icons/RotateLeft";

const useStyles = makeStyles((theme) => ({
  actionsOuter: {
    display: "flex",
    gap: theme.spacing(1),
    alignItems: "center",
  },
}));

export default function DaemonControlBox() {
  const classes = useStyles();
  const logs = useAppSelector((s) => s.rpc.logs);

  // The daemon can be manually started if it has failed or if it has not been started yet
  const canContextBeManuallyStarted = useAppSelector(
    (s) => s.rpc.status?.type === "Failed" || s.rpc.status === null,
  );
  const isContextInitializing = useAppSelector(
    (s) => s.rpc.status?.type === "Initializing",
  );

  const stringifiedDaemonStatus = useAppSelector((s) => s.rpc.status?.type ?? "not started");

  return (
    <InfoBox
      title={`Daemon Controller (${stringifiedDaemonStatus})`}
      mainContent={
        <CliLogsBox
          label="Logs (current session only)"
          logs={logs}
        />
      }
      additionalContent={
        <Box className={classes.actionsOuter}>
          <PromiseInvokeButton
            variant="contained"
            endIcon={<PlayArrowIcon />}
            onInvoke={initializeContext}
            requiresContext={false}
            disabled={!canContextBeManuallyStarted}
            isLoadingOverride={isContextInitializing}
            displayErrorSnackbar
          >
            Start Daemon
          </PromiseInvokeButton>
          <PromiseInvokeButton
            variant="contained"
            endIcon={<RotateLeftIcon />}
            onInvoke={relaunch}
            requiresContext={false}
            displayErrorSnackbar
          >
            Restart GUI
          </PromiseInvokeButton>
          <PromiseInvokeButton
            endIcon={<FolderOpenIcon />}
            isIconButton
            size="small"
            tooltipTitle="Open the data directory of the Swap Daemon in your file explorer"
            onInvoke={() => {
              // TODO: Implement this
              throw new Error("Not implemented");
            }}
          />
        </Box>
      }
      icon={null}
      loading={false}
    />
  );
}
