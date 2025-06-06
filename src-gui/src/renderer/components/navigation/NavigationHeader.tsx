import { Box, List, Badge } from "@mui/material";
import AccountBalanceWalletIcon from "@mui/icons-material/AccountBalanceWallet";
import HistoryOutlinedIcon from "@mui/icons-material/HistoryOutlined";
import SwapHorizOutlinedIcon from "@mui/icons-material/SwapHorizOutlined";
import FeedbackOutlinedIcon from "@mui/icons-material/FeedbackOutlined";
import RouteListItemIconButton from "./RouteListItemIconButton";
import UnfinishedSwapsBadge from "./UnfinishedSwapsCountBadge";
import { useTotalUnreadMessagesCount } from "store/hooks";
import SettingsIcon from "@mui/icons-material/Settings";

export default function NavigationHeader() {
  const totalUnreadCount = useTotalUnreadMessagesCount();

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
