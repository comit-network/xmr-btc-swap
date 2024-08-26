import { Box, List } from "@material-ui/core";
import AccountBalanceWalletIcon from "@material-ui/icons/AccountBalanceWallet";
import HelpOutlineIcon from "@material-ui/icons/HelpOutline";
import HistoryOutlinedIcon from "@material-ui/icons/HistoryOutlined";
import SwapHorizOutlinedIcon from "@material-ui/icons/SwapHorizOutlined";
import RouteListItemIconButton from "./RouteListItemIconButton";
import UnfinishedSwapsBadge from "./UnfinishedSwapsCountBadge";

export default function NavigationHeader() {
  return (
    <Box>
      <List>
        <RouteListItemIconButton name="Swap" route="/swap">
          <SwapHorizOutlinedIcon />
        </RouteListItemIconButton>
        <RouteListItemIconButton name="History" route="/history">
          <UnfinishedSwapsBadge>
            <HistoryOutlinedIcon />
          </UnfinishedSwapsBadge>
        </RouteListItemIconButton>
        <RouteListItemIconButton name="Wallet" route="/wallet">
          <AccountBalanceWalletIcon />
        </RouteListItemIconButton>
        <RouteListItemIconButton name="Help" route="/help">
          <HelpOutlineIcon />
        </RouteListItemIconButton>
      </List>
    </Box>
  );
}
