import { Box, Typography } from "@mui/material";

type Props = {
  children: React.ReactNode;
};

export default function MonospaceTextBox({ children }: Props) {
  return (
    <Box
      sx={(theme) => ({
        display: "flex",
        alignItems: "center",
        backgroundColor: theme.palette.grey[900],
        borderRadius: 2,
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
