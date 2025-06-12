import { Box, Typography } from "@mui/material";

export default function IconChip({
  icon,
  color,
  children,
}: {
  icon: React.ReactNode;
  color: string;
  children: React.ReactNode;
}) {
  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "row",
        gap: 1,
        backgroundColor: color,
        padding: 1,
        borderRadius: 10,
        alignItems: "center",
      }}
    >
      {icon}
      <Typography
        sx={{
          fontSize: 12,
          fontWeight: 500,
        }}
      >
        {children}
      </Typography>
    </Box>
  );
}
