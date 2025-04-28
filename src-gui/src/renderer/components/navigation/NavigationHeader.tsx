import { Box, List, Badge } from "@material-ui/core";
import AccountBalanceWalletIcon from "@material-ui/icons/AccountBalanceWallet";
import HistoryOutlinedIcon from "@material-ui/icons/HistoryOutlined";
import SwapHorizOutlinedIcon from "@material-ui/icons/SwapHorizOutlined";
import FeedbackOutlinedIcon from '@material-ui/icons/FeedbackOutlined';
import RouteListItemIconButton from "./RouteListItemIconButton";
import UnfinishedSwapsBadge from "./UnfinishedSwapsCountBadge";
import { useTotalUnreadMessagesCount } from "store/hooks";
import SettingsIcon from '@material-ui/icons/Settings';

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
