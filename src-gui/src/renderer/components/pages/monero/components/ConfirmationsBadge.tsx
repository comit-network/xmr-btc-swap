import { Box, Chip, Tooltip, Typography } from "@mui/material";
import {
  AutoAwesome as AutoAwesomeIcon,
  CheckCircleOutline as CheckCircleOutlineIcon,
} from "@mui/icons-material";

export default function ConfirmationsBadge({
  confirmations,
}: {
  confirmations: number;
}) {
  if (confirmations === 0) {
    return (
      <Chip
        icon={<AutoAwesomeIcon />}
        label="Published"
        color="secondary"
        size="small"
      />
    );
  } else if (confirmations < 10) {
    const label = (
      <>
        <Box
          sx={{
            display: "flex",
            flexDirection: "row",
            alignItems: "end",
            gap: 0.4,
          }}
        >
          <Typography variant="body2" sx={{ fontWeight: "bold" }}>
            {confirmations}
          </Typography>
          <Typography variant="caption">/10</Typography>
        </Box>
      </>
    );
    return <Chip label={label} color="warning" size="small" />;
  } else {
    return (
      <Tooltip title={`${confirmations} Confirmations`}>
        <CheckCircleOutlineIcon
          sx={{ color: "text.secondary" }}
          fontSize="small"
        />
      </Tooltip>
    );
  }
}
