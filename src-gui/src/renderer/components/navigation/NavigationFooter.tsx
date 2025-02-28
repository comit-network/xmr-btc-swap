import { Box, makeStyles, Tooltip } from "@material-ui/core";
import GitHubIcon from "@material-ui/icons/GitHub";
import RedditIcon from "@material-ui/icons/Reddit";
import DaemonStatusAlert from "../alert/DaemonStatusAlert";
import FundsLeftInWalletAlert from "../alert/FundsLeftInWalletAlert";
import MoneroWalletRpcUpdatingAlert from "../alert/MoneroWalletRpcUpdatingAlert";
import UnfinishedSwapsAlert from "../alert/UnfinishedSwapsAlert";
import DiscordIcon from "../icons/DiscordIcon";
import LinkIconButton from "../icons/LinkIconButton";
import { DISCORD_URL } from "../pages/help/ContactInfoBox";
import BackgroundRefundAlert from "../alert/BackgroundRefundAlert";
import MatrixIcon from "../icons/MatrixIcon";

const useStyles = makeStyles((theme) => ({
  outer: {
    display: "flex",
    flexDirection: "column",
    padding: theme.spacing(1),
    gap: theme.spacing(1),
  },
  linksOuter: {
    display: "flex",
    justifyContent: "space-evenly",
  },
}));

export default function NavigationFooter() {
  const classes = useStyles();

  return (
    <Box className={classes.outer}>
      <FundsLeftInWalletAlert />
      <UnfinishedSwapsAlert />
      <BackgroundRefundAlert />
      <DaemonStatusAlert />
      <MoneroWalletRpcUpdatingAlert />
      <Box className={classes.linksOuter}>
        <Tooltip title="Check out the GitHub repository">
          <span>
            <LinkIconButton url="https://github.com/UnstoppableSwap/unstoppableswap-gui">
              <GitHubIcon />
            </LinkIconButton>
          </span>
        </Tooltip>
        <Tooltip title="Join the Matrix room">
          <span>
            <LinkIconButton url="https://matrix.to/#/#unstoppableswap-space:matrix.org">
              <MatrixIcon />
            </LinkIconButton>
          </span>
        </Tooltip>
      </Box>
    </Box>
  );
}
