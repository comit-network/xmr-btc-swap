import { Box, Button } from "@mui/material";
import ApiAlertsBox from "renderer/components/pages/swap/ApiAlertsBox";
import { useState } from "react";
import SwapDialog from "./swap/SwapDialog";

export default function Dashboard() {
  const [showDialog, setShowDialog] = useState(false);

  return (
    <Box
      sx={{
        display: "flex",
        width: "100%",
        flexDirection: "column",
        alignItems: "center",
        paddingBottom: 1,
        gap: 1,
      }}
    >
      <ApiAlertsBox />
      <Button onClick={() => setShowDialog(true)}>Swap</Button>
      <SwapDialog open={showDialog} onClose={() => setShowDialog(false)} />
    </Box>
  );
}