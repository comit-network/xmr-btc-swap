import { Box, Typography } from "@mui/material";

type Props = {
  children: React.ReactNode;
  light?: boolean;
};

export default function MonospaceTextBox({ children, light = false }: Props) {
  return (
    <Box
      sx={(theme) => ({
        display: "flex",
        alignItems: "center",
        backgroundColor: light ? "transparent" : theme.palette.grey[900],
        borderRadius: 2,
        border: light ? `1px solid ${theme.palette.grey[800]}` : "none",
        padding: theme.spacing(1),
      })}
    >
      <Typography
        component="span"
        variant="overline"
        sx={{
          wordBreak: "break-word",
          whiteSpace: "pre-wrap",
          fontFamily: "monospace",
          lineHeight: 1.5,
          display: "flex",
          alignItems: "center",
        }}
      >
        {children}
      </Typography>
    </Box>
  );
}
