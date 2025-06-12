import { Box, TextField, Tooltip, Typography } from "@mui/material";
import ArrowForwardIcon from "@mui/icons-material/ArrowForward";

export default function SwapAmountSelector({
  fullWidth,
}: {
  fullWidth?: boolean;
}) {
  return (
    <Box
      sx={{
        display: "grid",
        gridTemplateColumns: "1fr auto 1fr",
        alignItems: "center",
        gap: 1,
        width: fullWidth ? "100%" : "auto",
      }}
    >
      <TextField
        label="BTC"
        fullWidth={fullWidth}
        sx={{
          gridColumn: "1 / 2",
          gridRow: "2",
        }}
      />
      <Typography
        variant="caption"
        sx={{
          gridColumn: "1 / 2",
          gridRow: "3",
        }}
      >
        (0.00 $)
      </Typography>

      <ArrowForwardIcon
        sx={{
          justifySelf: "center",
          gridColumn: "2 / 3",
          gridRow: "2",
        }}
      />

      <Tooltip
        title="The actual Monero amount might vary slightly"
        enterDelay={1500}
        enterNextDelay={500}
        leaveDelay={500}
      >
        <TextField
          label="XMR"
          fullWidth={fullWidth}
          sx={{
            gridColumn: "3 / 4",
            gridRow: "2",
          }}
        />
      </Tooltip>

      <Typography
        sx={{
          gridColumn: "3 / 4",
          gridRow: "3",
        }}
        variant="caption"
      >
        (0.00 $)
      </Typography>
    </Box>
  );
}
