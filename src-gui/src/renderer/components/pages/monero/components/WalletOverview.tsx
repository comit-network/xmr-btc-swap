import {
  Box,
  Typography,
  CircularProgress,
  Button,
  Card,
  CardContent,
  Divider,
  CardHeader,
  LinearProgress,
} from "@mui/material";
import { PiconeroAmount } from "../../../other/Units";
import { FiatPiconeroAmount } from "../../../other/Units";
import StateIndicator from "./StateIndicator";

interface WalletOverviewProps {
  balance?: {
    unlocked_balance: string;
    total_balance: string;
  };
  syncProgress?: {
    current_block: number;
    target_block: number;
    progress_percentage: number;
  };
}

// Component for displaying wallet address and balance
export default function WalletOverview({
  balance,
  syncProgress,
}: WalletOverviewProps) {
  const pendingBalance =
    parseFloat(balance.total_balance) - parseFloat(balance.unlocked_balance);

  const isSyncing = syncProgress && syncProgress.progress_percentage < 100;
  const blocksLeft = syncProgress?.target_block - syncProgress?.current_block;

  return (
    <Card sx={{ p: 2, position: "relative", borderRadius: 2 }} elevation={4}>
      {syncProgress && syncProgress.progress_percentage < 100 && (
        <LinearProgress
          value={syncProgress.progress_percentage}
          variant="determinate"
          sx={{
            width: "100%",
            position: "absolute",
            top: 0,
            left: 0,
          }}
        />
      )}

      {/* Balance */}
      <Box
        sx={{
          display: "grid",
          gridTemplateColumns: "1.5fr 1fr max-content",
          rowGap: 0.5,
          columnGap: 2,
          mb: 1,
        }}
      >
        <Typography
          variant="body2"
          color="text.secondary"
          sx={{ mb: 1, gridColumn: "1", gridRow: "1" }}
        >
          Available Funds
        </Typography>
        <Typography variant="h4" sx={{ gridColumn: "1", gridRow: "2" }}>
          <PiconeroAmount
            amount={parseFloat(balance.unlocked_balance)}
            fixedPrecision={4}
          />
        </Typography>
        <Typography
          variant="body2"
          color="text.secondary"
          sx={{ gridColumn: "1", gridRow: "3" }}
        >
          <FiatPiconeroAmount amount={parseFloat(balance.unlocked_balance)} />
        </Typography>
        {pendingBalance > 0 && (
          <>
            <Typography
              variant="body2"
              color="warning"
              sx={{
                mb: 1,
                animation: "pulse 2s infinite",
                gridColumn: "2",
                gridRow: "1",
                alignSelf: "end",
              }}
            >
              Pending
            </Typography>

            <Typography
              variant="h5"
              sx={{ gridColumn: "2", gridRow: "2", alignSelf: "center" }}
            >
              <PiconeroAmount amount={pendingBalance} fixedPrecision={4} />
            </Typography>
            <Typography
              variant="body2"
              color="text.secondary"
              sx={{ gridColumn: "2", gridRow: "3" }}
            >
              <FiatPiconeroAmount amount={pendingBalance} />
            </Typography>
          </>
        )}

        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            alignItems: "flex-end",
          }}
        >
          <Box
            sx={{
              display: "flex",
              flexDirection: "row",
              alignItems: "center",
              gap: 1,
            }}
          >
            <Typography variant="body2">
              {isSyncing ? "syncing" : "synced"}
            </Typography>
            <StateIndicator
              color={isSyncing ? "primary" : "success"}
              pulsating={isSyncing}
            />
          </Box>
          {isSyncing && (
            <Typography variant="body2" color="text.secondary">
              {blocksLeft.toLocaleString()} blocks left
            </Typography>
          )}
        </Box>
      </Box>
    </Card>
  );
}
