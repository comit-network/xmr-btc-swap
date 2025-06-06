import { Button, Paper, Typography } from "@mui/material";

export default function PaperTextBox({ stdOut }: { stdOut: string }) {
  function handleCopyLogs() {
    throw new Error("Not implemented");
  }

  return (
    <Paper
      variant="outlined"
      sx={{
        overflow: "auto",
        padding: 1,
        marginTop: 1,
        marginBottom: 1,
        maxHeight: "10rem",
      }}
    >
      <Typography component="pre" variant="body2">
        {stdOut}
      </Typography>
      <Button onClick={handleCopyLogs} sx={{ marginTop: 1 }}>
        Copy
      </Button>
    </Paper>
  );
}
