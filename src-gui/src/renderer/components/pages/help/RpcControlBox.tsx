import { Box, makeStyles } from "@material-ui/core";
import FolderOpenIcon from "@material-ui/icons/FolderOpen";
import PlayArrowIcon from "@material-ui/icons/PlayArrow";
import StopIcon from "@material-ui/icons/Stop";
import { RpcProcessStateType } from "models/rpcModel";
import IpcInvokeButton from "renderer/components/IpcInvokeButton";
import { useAppSelector } from "store/hooks";
import InfoBox from "../../modal/swap/InfoBox";
import CliLogsBox from "../../other/RenderedCliLog";

const useStyles = makeStyles((theme) => ({
  actionsOuter: {
    display: "flex",
    gap: theme.spacing(1),
    alignItems: "center",
  },
}));

export default function RpcControlBox() {
  const rpcProcess = useAppSelector((state) => state.rpc.process);
  const isRunning =
    rpcProcess.type === RpcProcessStateType.STARTED ||
    rpcProcess.type === RpcProcessStateType.LISTENING_FOR_CONNECTIONS;
  const classes = useStyles();

  return (
    <InfoBox
      title={`Swap Daemon (${rpcProcess.type})`}
      mainContent={
        isRunning || rpcProcess.type === RpcProcessStateType.EXITED ? (
          <CliLogsBox
            label="Swap Daemon Logs (current session only)"
            logs={rpcProcess.logs}
          />
        ) : null
      }
      additionalContent={
        <Box className={classes.actionsOuter}>
          <IpcInvokeButton
            variant="contained"
            ipcChannel="spawn-start-rpc"
            ipcArgs={[]}
            endIcon={<PlayArrowIcon />}
            disabled={isRunning}
            requiresRpc={false}
          >
            Start Daemon
          </IpcInvokeButton>
          <IpcInvokeButton
            variant="contained"
            ipcChannel="stop-cli"
            ipcArgs={[]}
            endIcon={<StopIcon />}
            disabled={!isRunning}
            requiresRpc={false}
          >
            Stop Daemon
          </IpcInvokeButton>
          <IpcInvokeButton
            ipcChannel="open-data-dir-in-file-explorer"
            ipcArgs={[]}
            endIcon={<FolderOpenIcon />}
            requiresRpc={false}
            isIconButton
            size="small"
            tooltipTitle="Open the data directory of the Swap Daemon in your file explorer"
          />
        </Box>
      }
      icon={null}
      loading={false}
    />
  );
}
