import { Box } from "@mui/material";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { useAppSelector } from "store/hooks";
import InfoBox from "renderer/components/pages/swap/swap/components/InfoBox";
import CliLogsBox from "renderer/components/other/RenderedCliLog";
import { getDataDir, initializeContext } from "renderer/rpc";
import { relaunch } from "@tauri-apps/plugin-process";
import RotateLeftIcon from "@mui/icons-material/RotateLeft";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { TauriContextStatusEvent } from "models/tauriModel";

export default function DaemonControlBox() {
  const logs = useAppSelector((s) => s.rpc.logs);

  // The daemon can be manually started if it has failed or if it has not been started yet
  const canContextBeManuallyStarted = useAppSelector(
    (s) =>
      s.rpc.status === TauriContextStatusEvent.Failed || s.rpc.status === null,
  );
  const isContextInitializing = useAppSelector(
    (s) => s.rpc.status === TauriContextStatusEvent.Initializing,
  );

  const stringifiedDaemonStatus = useAppSelector(
    (s) => s.rpc.status ?? "not started",
  );

  return (
    <InfoBox
      id="daemon-control-box"
      title={`Daemon Controller (${stringifiedDaemonStatus})`}
      mainContent={
        <CliLogsBox label="Logs (current session only)" logs={logs} />
      }
      additionalContent={
        <Box sx={{ display: "flex", gap: 1, alignItems: "center" }}>
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
            requiresContext={false}
            size="small"
            tooltipTitle="Open the data directory in your file explorer"
            onInvoke={async () => {
              const dataDir = await getDataDir();
              await revealItemInDir(dataDir);
            }}
          />
        </Box>
      }
      icon={null}
      loading={false}
    />
  );
}
