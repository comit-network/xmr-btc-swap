import { Box, Tooltip } from "@mui/material";
import GitHubIcon from "@mui/icons-material/GitHub";
import DaemonStatusAlert, {
  BackgroundProgressAlerts,
} from "../alert/DaemonStatusAlert";
import FundsLeftInWalletAlert from "../alert/FundsLeftInWalletAlert";
import UnfinishedSwapsAlert from "../alert/UnfinishedSwapsAlert";
import LinkIconButton from "../icons/LinkIconButton";
import BackgroundRefundAlert from "../alert/BackgroundRefundAlert";
import MatrixIcon from "../icons/MatrixIcon";
import { MenuBook } from "@mui/icons-material";

export default function NavigationFooter() {
  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        padding: 1,
        gap: 1,
      }}
    >
      <FundsLeftInWalletAlert />
      <UnfinishedSwapsAlert />
      <BackgroundRefundAlert />
      <DaemonStatusAlert />
      <BackgroundProgressAlerts />
      <Box
        sx={{
          display: "flex",
          justifyContent: "space-evenly",
        }}
      >
        <Tooltip title="Check out the GitHub repository">
          <span>
            <LinkIconButton url="https://github.com/UnstoppableSwap/core">
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
