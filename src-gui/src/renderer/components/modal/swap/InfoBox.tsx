import { Box, LinearProgress, Paper, Typography } from "@mui/material";
import { ReactNode } from "react";

type Props = {
  id?: string;
  title: ReactNode | null;
  mainContent: ReactNode;
  additionalContent: ReactNode;
  loading: boolean;
  icon: ReactNode;
};

export default function InfoBox({
  id = null,
  title,
  mainContent,
  additionalContent,
  icon,
  loading,
}: Props) {
  return (
    <Paper
      variant="outlined"
      id={id}
      sx={{
        padding: 1.5,
        overflow: "hidden",
        display: "flex",
        flexDirection: "column",
        gap: 1,
      }}
    >
      {title ? <Typography variant="subtitle1">{title}</Typography> : null}
      <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
        {icon}
        {mainContent}
      </Box>
      {loading ? <LinearProgress variant="indeterminate" /> : null}
      <Box>{additionalContent}</Box>
    </Paper>
  );
}
