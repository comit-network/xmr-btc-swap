import { Box, DialogTitle, Typography } from "@mui/material";
import DebugPageSwitchBadge from "./pages/DebugPageSwitchBadge";
import FeedbackSubmitBadge from "./pages/FeedbackSubmitBadge";
import TorStatusBadge from "./pages/TorStatusBadge";

export default function SwapDialogTitle({
  title,
  debug,
  setDebug,
}: {
  title: string;
  debug: boolean;
  setDebug: (d: boolean) => void;
}) {
  return (
    <DialogTitle
      sx={{
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
      }}
    >
      <Typography variant="h6">{title}</Typography>
      <Box sx={{ display: "flex", alignItems: "center", gridGap: 1 }}>
        <FeedbackSubmitBadge />
        <DebugPageSwitchBadge enabled={debug} setEnabled={setDebug} />
        <TorStatusBadge />
      </Box>
    </DialogTitle>
  );
}
