import {
  Box,
  Button,
  Checkbox,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControlLabel,
  IconButton,
  Paper,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import { ErrorOutline, Visibility } from "@mui/icons-material";
import ExternalLink from "renderer/components/other/ExternalLink";
import SwapSelectDropDown from "./SwapSelectDropDown";
import LogViewer from "./LogViewer";
import { useFeedback, MAX_FEEDBACK_LENGTH } from "./useFeedback";
import { useState } from "react";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";

export default function FeedbackDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [swapLogsEditorOpen, setSwapLogsEditorOpen] = useState(false);
  const [daemonLogsEditorOpen, setDaemonLogsEditorOpen] = useState(false);

  const { input, setInputState, logs, error, clearState, submitFeedback } =
    useFeedback();

  const handleClose = () => {
    clearState();
    onClose();
  };

  const bodyTooLong = input.bodyText.length > MAX_FEEDBACK_LENGTH;

  return (
    <Dialog open={open} onClose={handleClose}>
      <DialogTitle style={{ paddingBottom: "0.5rem" }}>
        Submit Feedback
      </DialogTitle>
      <DialogContent>
        <Box
          style={{
            display: "flex",
            flexDirection: "column",
            gap: "1.5rem",
          }}
        >
          {error && (
            <Box
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "start",
                gap: "0.5rem",
                width: "100%",
                backgroundColor: "hsla(0, 45%, 17%, 1)",
                padding: "0.5rem",
                borderRadius: "0.5rem",
                border: "1px solid hsla(0, 61%, 32%, 1)",
              }}
            >
              <ErrorOutline style={{ color: "hsla(0, 77%, 75%, 1)" }} />
              <Typography style={{ color: "hsla(0, 83%, 91%, 1)" }} noWrap>
                {error}
              </Typography>
            </Box>
          )}
          <Box>
            <Typography style={{ marginBottom: "0.5rem" }}>
              Have a question or need assistance? Message us below or{" "}
              <ExternalLink href="https://docs.unstoppableswap.net/send_feedback#email-support">
                email us
              </ExternalLink>
              !
            </Typography>
            <TextField
              variant="outlined"
              value={input.bodyText}
              onChange={(e) =>
                setInputState((prev) => ({
                  ...prev,
                  bodyText: e.target.value,
                }))
              }
              label={
                bodyTooLong
                  ? `Text is too long (${input.bodyText.length}/${MAX_FEEDBACK_LENGTH})`
                  : "Message"
              }
              multiline
              minRows={4}
              maxRows={4}
              fullWidth
              error={bodyTooLong}
            />
          </Box>
          <Box>
            <Typography style={{ marginBottom: "0.5rem" }}>
              Attach logs with your feedback for better support.
            </Typography>
            <Box
              style={{
                display: "flex",
                flexDirection: "row",
                justifyContent: "space-between",
                gap: "1rem",
                paddingBottom: "0.5rem",
              }}
            >
              <SwapSelectDropDown
                selectedSwap={input.selectedSwap}
                setSelectedSwap={(swapId) =>
                  setInputState((prev) => ({
                    ...prev,
                    selectedSwap: swapId,
                  }))
                }
              />
              <Tooltip title="View the logs">
                <Box
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                  }}
                >
                  <IconButton
                    onClick={() => setSwapLogsEditorOpen(true)}
                    disabled={input.selectedSwap === null}
                    size="large"
                  >
                    <Visibility />
                  </IconButton>
                </Box>
              </Tooltip>
            </Box>
            <LogViewer
              open={swapLogsEditorOpen}
              setOpen={setSwapLogsEditorOpen}
              logs={logs.swapLogs}
              setIsRedacted={(redact) =>
                setInputState((prev) => ({
                  ...prev,
                  isSwapLogsRedacted: redact,
                }))
              }
              isRedacted={input.isSwapLogsRedacted}
            />
            <Box
              style={{
                display: "flex",
                flexDirection: "row",
                justifyContent: "space-between",
                gap: "1rem",
              }}
            >
              <Paper
                variant="outlined"
                style={{ padding: "0.5rem", width: "100%" }}
              >
                <FormControlLabel
                  control={
                    <Checkbox
                      color="primary"
                      checked={input.attachDaemonLogs}
                      onChange={(e) =>
                        setInputState((prev) => ({
                          ...prev,
                          attachDaemonLogs: e.target.checked,
                        }))
                      }
                    />
                  }
                  label="Attach logs from the current session"
                />
              </Paper>
              <Tooltip title="View the logs">
                <Box
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                  }}
                >
                  <IconButton
                    onClick={() => setDaemonLogsEditorOpen(true)}
                    disabled={input.attachDaemonLogs === false}
                    size="large"
                  >
                    <Visibility />
                  </IconButton>
                </Box>
              </Tooltip>
            </Box>
          </Box>
          <Typography
            variant="caption"
            color="textSecondary"
            style={{ marginBottom: "0.5rem" }}
          >
            Your feedback will be answered in the app and can be found in the
            Feedback tab
          </Typography>
          <LogViewer
            open={daemonLogsEditorOpen}
            setOpen={setDaemonLogsEditorOpen}
            logs={logs.daemonLogs}
            setIsRedacted={(redact) =>
              setInputState((prev) => ({
                ...prev,
                isDaemonLogsRedacted: redact,
              }))
            }
            isRedacted={input.isDaemonLogsRedacted}
          />
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={handleClose}>Cancel</Button>
        <PromiseInvokeButton
          requiresContext={false}
          color="primary"
          variant="contained"
          onInvoke={submitFeedback}
          onSuccess={handleClose}
        >
          Submit
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}
