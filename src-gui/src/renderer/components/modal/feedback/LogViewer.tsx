import {
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  Paper,
  Switch,
  Typography,
} from "@mui/material";
import { CliLog } from "models/cliModel";
import CliLogsBox from "renderer/components/other/RenderedCliLog";

interface LogViewerProps {
  open: boolean;
  setOpen: (_: boolean) => void;
  logs: (string | CliLog)[] | null;
  setIsRedacted: (_: boolean) => void;
  isRedacted: boolean;
}

export default function LogViewer({
  open,
  setOpen,
  logs,
  setIsRedacted,
  isRedacted,
}: LogViewerProps) {
  return (
    <Dialog open={open} onClose={() => setOpen(false)} fullWidth>
      <DialogContent>
        <Box>
          <DialogContentText>
            <Box
              style={{
                display: "flex",
                flexDirection: "row",
                alignItems: "center",
              }}
            >
              <Typography>
                These are the logs that would be attached to your feedback
                message and provided to us developers. They help us narrow down
                the problem you encountered.
              </Typography>
            </Box>
          </DialogContentText>

          <CliLogsBox
            label="Logs"
            logs={logs}
            topRightButton={
              <Paper
                style={{
                  display: "flex",
                  justifyContent: "flex-end",
                  alignItems: "center",
                  paddingLeft: "0.5rem",
                }}
                variant="outlined"
              >
                Redact
                <Switch
                  color="primary"
                  checked={isRedacted}
                  onChange={(_, checked: boolean) => setIsRedacted(checked)}
                />
              </Paper>
            }
          />
        </Box>
      </DialogContent>
      <DialogActions>
        <Button
          variant="contained"
          color="primary"
          onClick={() => setOpen(false)}
        >
          Close
        </Button>
      </DialogActions>
    </Dialog>
  );
}
