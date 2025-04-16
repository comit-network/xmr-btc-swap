import { Box, makeStyles, Tooltip } from "@material-ui/core";
import GitHubIcon from "@material-ui/icons/GitHub";
import DaemonStatusAlert from "../alert/DaemonStatusAlert";
import FundsLeftInWalletAlert from "../alert/FundsLeftInWalletAlert";
import MoneroWalletRpcUpdatingAlert from "../alert/MoneroWalletRpcUpdatingAlert";
import UnfinishedSwapsAlert from "../alert/UnfinishedSwapsAlert";
import LinkIconButton from "../icons/LinkIconButton";
import BackgroundRefundAlert from "../alert/BackgroundRefundAlert";
import MatrixIcon from "../icons/MatrixIcon";
import { BookRounded, MenuBook } from "@material-ui/icons";

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
        <Tooltip title="Read our official documentation">
          <span>
            <LinkIconButton url="https://docs.unstoppableswap.net">
              <MenuBook />
            </LinkIconButton>
          </span>
        </Tooltip>
      </Box>
    </Box>
  );
}
