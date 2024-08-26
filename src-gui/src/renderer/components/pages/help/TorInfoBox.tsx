import { Box, makeStyles, Typography } from "@material-ui/core";
import PlayArrowIcon from "@material-ui/icons/PlayArrow";
import StopIcon from "@material-ui/icons/Stop";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { useAppSelector } from "store/hooks";
import InfoBox from "../../modal/swap/InfoBox";
import CliLogsBox from "../../other/RenderedCliLog";

const useStyles = makeStyles((theme) => ({
  actionsOuter: {
    display: "flex",
    gap: theme.spacing(1),
  },
}));

export default function TorInfoBox() {
  const isTorRunning = useAppSelector((state) => state.tor.processRunning);
  const torStdOut = useAppSelector((s) => s.tor.stdOut);
  const classes = useStyles();

  return (
    <InfoBox
      title="Tor (The Onion Router)"
      mainContent={
        <Box
          style={{
            width: "100%",
            display: "flex",
            flexDirection: "column",
            gap: "8px",
          }}
        >
          <Typography variant="subtitle2">
            Tor is a network that allows you to anonymously connect to the
            internet. It is a free and open network that is operated by
            volunteers. You can start and stop Tor by clicking the buttons
            below. If Tor is running, all traffic will be routed through it and
            the swap provider will not be able to see your IP address.
          </Typography>
          <CliLogsBox label="Tor Daemon Logs" logs={torStdOut.split("\n")} />
        </Box>
      }
      additionalContent={
        <Box className={classes.actionsOuter}>
          <PromiseInvokeButton
            variant="contained"
            disabled={isTorRunning}
            endIcon={<PlayArrowIcon />}
            onClick={() => {
              throw new Error("Not implemented");
            }}
          >
            Start Tor
          </PromiseInvokeButton>
          <PromiseInvokeButton
            variant="contained"
            disabled={!isTorRunning}
            endIcon={<StopIcon />}
            onClick={() => {
              throw new Error("Not implemented");
            }}
          >
            Stop Tor
          </PromiseInvokeButton>
        </Box>
      }
      icon={null}
      loading={false}
    />
  );
}
