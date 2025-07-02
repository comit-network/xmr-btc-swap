import { Box, Typography, LinearProgress, Paper } from "@mui/material";
import { usePendingBackgroundProcesses } from "store/hooks";

export default function MakerDiscoveryStatus() {
  const backgroundProcesses = usePendingBackgroundProcesses();

  // Find active ListSellers processes
  const listSellersProcesses = backgroundProcesses.filter(
    ([, status]) =>
      status.componentName === "ListSellers" &&
      status.progress.type === "Pending",
  );

  const isActive = listSellersProcesses.length > 0;

  // Default values for inactive state
  let progress = {
    rendezvous_points_total: 0,
    peers_discovered: 0,
    rendezvous_points_connected: 0,
    quotes_received: 0,
    quotes_failed: 0,
  };
  let progressValue = 0;

  if (isActive) {
    // Use the first ListSellers process for display
    const [, status] = listSellersProcesses[0];

    // Type guard to ensure we have ListSellers progress
    if (
      status.componentName === "ListSellers" &&
      status.progress.type === "Pending"
    ) {
      progress = status.progress.content;

      const totalExpected =
        progress.rendezvous_points_total + progress.peers_discovered;
      const totalCompleted =
        progress.rendezvous_points_connected +
        progress.quotes_received +
        progress.quotes_failed;
      progressValue =
        totalExpected > 0 ? (totalCompleted / totalExpected) * 100 : 0;
    }
  }

  return (
    <Paper
      variant="outlined"
      sx={{
        width: "100%",
        mb: 2,
        p: 2,
        border: "1px solid",
        borderColor: isActive ? "success.main" : "divider",
        borderRadius: 1,
        opacity: isActive ? 1 : 0.6,
      }}
    >
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          gap: 1.5,
          width: "100%",
        }}
      >
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            width: "100%",
          }}
        >
          <Typography
            variant="body2"
            sx={{
              fontWeight: "medium",
              color: isActive ? "info.main" : "text.disabled",
            }}
          >
            {isActive
              ? "Getting offers..."
              : "Waiting a few seconds before refreshing offers"}
          </Typography>
          <Box sx={{ display: "flex", gap: 2 }}>
            <Typography
              variant="caption"
              sx={{
                color: isActive ? "success.main" : "text.disabled",
                fontWeight: "medium",
              }}
            >
              {progress.quotes_received} online
            </Typography>
            <Typography
              variant="caption"
              sx={{
                color: isActive ? "error.main" : "text.disabled",
                fontWeight: "medium",
              }}
            >
              {progress.quotes_failed} offline
            </Typography>
          </Box>
        </Box>
        <LinearProgress
          variant="determinate"
          value={Math.min(progressValue, 100)}
          sx={{
            width: "100%",
            height: 8,
            borderRadius: 4,
            opacity: isActive ? 1 : 0.4,
          }}
        />
      </Box>
    </Paper>
  );
}
