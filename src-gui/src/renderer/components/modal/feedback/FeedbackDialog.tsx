import {
  Box,
  Button,
  Checkbox,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  FormControlLabel,
  IconButton,
  MenuItem,
  Paper,
  Select,
  Switch,
  TextField,
  Tooltip,
  Typography,
} from "@material-ui/core";
import { useSnackbar } from "notistack";
import { useEffect, useState } from "react";
import TruncatedText from "renderer/components/other/TruncatedText";
import { store } from "renderer/store/storeRenderer";
import { useActiveSwapInfo, useAppSelector } from "store/hooks";
import { logsToRawString, parseDateString } from "utils/parseUtils";
import { submitFeedbackViaHttp, AttachmentInput } from "../../../api";
import LoadingButton from "../../other/LoadingButton";
import { PiconeroAmount } from "../../other/Units";
import { getLogsOfSwap, redactLogs } from "renderer/rpc";
import logger from "utils/logger";
import { Label, Visibility } from "@material-ui/icons";
import CliLogsBox from "renderer/components/other/RenderedCliLog";
import { CliLog, parseCliLogString } from "models/cliModel";
import { addFeedbackId } from "store/features/conversationsSlice";

async function submitFeedback(body: string, swapId: string | null, swapLogs: string | null, daemonLogs: string | null) {
  const attachments: AttachmentInput[] = [];

  if (swapId !== null) {
    const swapInfo = store.getState().rpc.state.swapInfos[swapId];
    if (swapInfo) {
      // Add swap info as an attachment
      attachments.push({
        key: `swap_info_${swapId}.json`,
        content: JSON.stringify(swapInfo, null, 2), // Pretty print JSON
      });
      // Retrieve and add logs for the specific swap
      try {
          const logs = await getLogsOfSwap(swapId, false);
          attachments.push({
            key: `swap_logs_${swapId}.txt`,
            content: logs.logs.map((l) => JSON.stringify(l)).join("\n"),
          });
      } catch (logError) {
          logger.error(logError, "Failed to get logs for swap", { swapId });
          // Optionally add an attachment indicating log retrieval failure
          attachments.push({ key: `swap_logs_${swapId}.error`, content: "Failed to retrieve swap logs." });
      }
    } else {
      logger.warn("Selected swap info not found in state", { swapId });
      attachments.push({ key: `swap_info_${swapId}.error`, content: "Swap info not found." });
    }

    // Add swap logs as an attachment
    if (swapLogs) {
      attachments.push({
        key: `swap_logs_${swapId}.txt`,
        content: swapLogs,
      });
    }
  }

  // Handle daemon logs
  if (daemonLogs !== null) {
    attachments.push({
      key: "daemon_logs.txt",
      content: daemonLogs,
    });
  }

  // Call the updated API function
  const feedbackId = await submitFeedbackViaHttp(body, attachments);
  
  // Dispatch only the ID
  store.dispatch(addFeedbackId(feedbackId));
}

/*
 * This component is a dialog that allows the user to submit feedback to the
 * developers. The user can enter a message and optionally attach logs from a
 * specific swap.
 * selectedSwap = null means no swap is attached
 */
function SwapSelectDropDown({
  selectedSwap,
  setSelectedSwap,
}: {
  selectedSwap: string | null;
  setSelectedSwap: (swapId: string | null) => void;
}) {
  const swaps = useAppSelector((state) =>
    Object.values(state.rpc.state.swapInfos),
  );

  return (
    <Select
      value={selectedSwap ?? ""}
      variant="outlined"
      onChange={(e) => setSelectedSwap(e.target.value as string || null)}
      style={{ width: "100%" }}
      displayEmpty
    >
      <MenuItem value="">Do not attach a swap</MenuItem>
      {swaps.map((swap) => (
        <MenuItem value={swap.swap_id} key={swap.swap_id}>
          Swap{" "}<TruncatedText>{swap.swap_id}</TruncatedText>{" "}from{" "}
          {new Date(parseDateString(swap.start_date)).toDateString()} (
          <PiconeroAmount amount={swap.xmr_amount} />)
        </MenuItem>
      ))}
    </Select>
  );
}

const MAX_FEEDBACK_LENGTH = 4000;

export default function FeedbackDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [pending, setPending] = useState(false);
  const [bodyText, setBodyText] = useState("");
  const currentSwapId = useActiveSwapInfo();

  const { enqueueSnackbar } = useSnackbar();

  const [selectedSwap, setSelectedSwap] = useState<
    string | null
  >(currentSwapId?.swap_id || null);
  const [swapLogs, setSwapLogs] = useState<(string | CliLog)[] | null>(null);
  const [attachDaemonLogs, setAttachDaemonLogs] = useState(true);

  const [daemonLogs, setDaemonLogs] = useState<(string | CliLog)[] | null>(null);

  useEffect(() => {
    // Reset logs if no swap is selected
    if (selectedSwap === null) {
      setSwapLogs(null);
      return;
    }

    // Fetch the logs from the rust backend and update the state
    getLogsOfSwap(selectedSwap, false).then((response) => setSwapLogs(response.logs.map(parseCliLogString)))
  }, [selectedSwap]);

  useEffect(() => {
    if (attachDaemonLogs === false) {
      setDaemonLogs(null);
      return;
    }

    setDaemonLogs(store.getState().rpc?.logs)
  }, [attachDaemonLogs]);

  // Whether to display the log editor
  const [swapLogsEditorOpen, setSwapLogsEditorOpen] = useState(false);
  const [daemonLogsEditorOpen, setDaemonLogsEditorOpen] = useState(false);

  const bodyTooLong = bodyText.length > MAX_FEEDBACK_LENGTH;

  const clearState = () => {
    setBodyText("");
    setAttachDaemonLogs(false);
    setSelectedSwap(null);
  }

  const sendFeedback = async () => {
    if (pending) {
      return;
    }

    try {
      setPending(true);
      await submitFeedback(
        bodyText, 
        selectedSwap, 
        logsToRawString(swapLogs ?? []), 
        logsToRawString(daemonLogs ?? [])
      );
      enqueueSnackbar("Feedback submitted successfully!", {
        variant: "success",
      });
      clearState()
    } catch (e) {
      logger.error(`Failed to submit feedback: ${e}`);
      enqueueSnackbar(`Failed to submit feedback (${e})`, {
        variant: "error",
      });
    } finally {
      setPending(false);
    }
    onClose();
  }

  const setSwapLogsRedacted = async (redact: boolean) => {
    setSwapLogs((await getLogsOfSwap(selectedSwap, redact)).logs.map(parseCliLogString))
  }

  const setDaemonLogsRedacted = async (redact: boolean) => {
    if (!redact)
      return setDaemonLogs(store.getState().rpc?.logs)

    const redactedLogs = await redactLogs(daemonLogs);
    setDaemonLogs(redactedLogs)
  }

  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>Submit Feedback</DialogTitle>
      <DialogContent>
        <ul>
          <li>Got something to say? Drop us a message below. </li>
          <li>If you had an issue with a specific swap, select it from the dropdown to attach the logs.
            It will help us figure out what went wrong.
          </li>
          <li>We appreciate you taking the time to share your thoughts! Every message is read by a core developer!</li>
        </ul>
        <Box
          style={{
            display: "flex",
            flexDirection: "column",
            gap: "1rem",
          }}
        >
          <TextField
            variant="outlined"
            value={bodyText}
            onChange={(e) => setBodyText(e.target.value)}
            label={
              bodyTooLong
                ? `Text is too long (${bodyText.length}/${MAX_FEEDBACK_LENGTH})`
                : "Message"
            }
            multiline
            minRows={4}
            maxRows={4}
            fullWidth
            error={bodyTooLong}
          />
          <Box style={{
            display: "flex",
            flexDirection: "row",
            justifyContent: "space-between",
            gap: "1rem",
          }}>

            <SwapSelectDropDown
              selectedSwap={selectedSwap}
              setSelectedSwap={setSelectedSwap}
            />
            <Tooltip title="View the logs">
              <Box style={{ display: "flex", alignItems: "center", justifyContent: "center" }}>
                <IconButton onClick={() => setSwapLogsEditorOpen(true)} disabled={selectedSwap === null}>
                  <Visibility />
                </IconButton>
              </Box>
            </Tooltip>
          </Box>
          <LogViewer open={swapLogsEditorOpen} setOpen={setSwapLogsEditorOpen} logs={swapLogs} redact={setSwapLogsRedacted} />
          <Box style={{
            display: "flex",
            flexDirection: "row",
            justifyContent: "space-between",
            gap: "1rem",
          }}>
            <Paper variant="outlined" style={{ padding: "0.5rem", width: "100%" }} >
              <FormControlLabel
                control={
                  <Checkbox
                    color="primary"
                    checked={attachDaemonLogs}
                    onChange={(e) => setAttachDaemonLogs(e.target.checked)}
                  />
                }
                label="Attach logs from the current session"
              />
            </Paper>
            <Tooltip title="View the logs">
              <Box style={{ display: "flex", alignItems: "center", justifyContent: "center" }}>
                <IconButton onClick={() => setDaemonLogsEditorOpen(true)} disabled={attachDaemonLogs === false}>
                  <Visibility />
                </IconButton>
              </Box>
            </Tooltip>
          </Box>
          <LogViewer open={daemonLogsEditorOpen} setOpen={setDaemonLogsEditorOpen} logs={daemonLogs} redact={setDaemonLogsRedacted} />
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={() => { clearState(); onClose() }}>Cancel</Button>
        <LoadingButton
          color="primary"
          variant="contained"
          onClick={sendFeedback}
          loading={pending}
        >
          Submit
        </LoadingButton>
      </DialogActions>
    </Dialog>
  );
}

function LogViewer(
  { open,
    setOpen,
    logs,
    redact
  }: {
    open: boolean,
    setOpen: (_: boolean) => void,
    logs: (string | CliLog)[] | null,
    redact: (_: boolean) => void
  }) {
  return (
    <Dialog open={open} onClose={() => setOpen(false)} fullWidth>
      <DialogContent>
        <Box>
          <DialogContentText>
            <Box style={{ display: "flex", flexDirection: "row", alignItems: "center" }}>
              <Typography>
                These are the logs that would be attached to your feedback message and provided to us developers.
                They help us narrow down the problem you encountered.
              </Typography>


            </Box>
          </DialogContentText>

          <CliLogsBox label="Logs" logs={logs} topRightButton={<Paper style={{ display: 'flex', justifyContent: 'flex-end', alignItems: 'center', paddingLeft: "0.5rem" }} variant="outlined">
            Redact
            <Switch color="primary" onChange={(_, checked: boolean) => redact(checked)} />
          </Paper>} />
        </Box>
      </DialogContent>
      <DialogActions>
        <Button variant="contained" color="primary" onClick={() => setOpen(false)}>
          Close
        </Button>
      </DialogActions>
    </Dialog >
  )
}