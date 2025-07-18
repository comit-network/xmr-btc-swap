import { Box, List, Badge } from "@mui/material";
import AccountBalanceWalletIcon from "@mui/icons-material/AccountBalanceWallet";
import HistoryOutlinedIcon from "@mui/icons-material/HistoryOutlined";
import SwapHorizOutlinedIcon from "@mui/icons-material/SwapHorizOutlined";
import FeedbackOutlinedIcon from "@mui/icons-material/FeedbackOutlined";
import RouteListItemIconButton from "./RouteListItemIconButton";
import UnfinishedSwapsBadge from "./UnfinishedSwapsCountBadge";
import { useIsSwapRunning, useTotalUnreadMessagesCount } from "store/hooks";
import SettingsIcon from "@mui/icons-material/Settings";
import AttachMoneyIcon from "@mui/icons-material/AttachMoney";
import BitcoinIcon from "../icons/BitcoinIcon";
import MoneroIcon from "../icons/MoneroIcon";

export default function NavigationHeader() {
  const totalUnreadCount = useTotalUnreadMessagesCount();
  const isSwapRunning = useIsSwapRunning();

  return (
    <Box>
      <List>
        <RouteListItemIconButton name="Wallet" route={["/monero-wallet", "/"]}>
          <MoneroIcon />
        </RouteListItemIconButton>
        <RouteListItemIconButton name="Wallet" route="/bitcoin-wallet">
          <BitcoinIcon />
        </RouteListItemIconButton>
        <RouteListItemIconButton name="Swap" route={["/swap"]}>
          <Badge invisible={!isSwapRunning} variant="dot" color="primary">
            <SwapHorizOutlinedIcon />
          </Badge>
        </RouteListItemIconButton>
        <RouteListItemIconButton name="History" route="/history">
          <UnfinishedSwapsBadge>
            <HistoryOutlinedIcon />
          </UnfinishedSwapsBadge>
        </RouteListItemIconButton>
        <RouteListItemIconButton name="Feedback" route="/feedback">
          <Badge
            badgeContent={totalUnreadCount}
            color="primary"
            overlap="rectangular"
            invisible={totalUnreadCount === 0}
          >
            <FeedbackOutlinedIcon />
          </Badge>
        </RouteListItemIconButton>
        <RouteListItemIconButton name="Settings" route="/settings">
          <SettingsIcon />
        </RouteListItemIconButton>
      </List>
    </Box>
  );
}
