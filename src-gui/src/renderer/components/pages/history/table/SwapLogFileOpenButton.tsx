import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
} from "@mui/material";
import { ButtonProps } from "@mui/material/Button";
import { CliLog, parseCliLogString } from "models/cliModel";
import { GetLogsResponse } from "models/tauriModel";
import { useState } from "react";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { getLogsOfSwap } from "renderer/rpc";
import CliLogsBox from "../../../other/RenderedCliLog";

export default function SwapLogFileOpenButton({
  swapId,
  ...props
}: { swapId: string } & ButtonProps) {
  const [logs, setLogs] = useState<(CliLog | string)[] | null>(null);

  function onLogsReceived(response: GetLogsResponse) {
    setLogs(response.logs.map(parseCliLogString));
  }

  return (
    <>
      <PromiseInvokeButton
        onSuccess={onLogsReceived}
        onInvoke={() => getLogsOfSwap(swapId, false)}
        {...props}
      >
        View full logs
      </PromiseInvokeButton>
      <PromiseInvokeButton
        onSuccess={onLogsReceived}
        onInvoke={() => getLogsOfSwap(swapId, true)}
        {...props}
      >
        View redacted logs
      </PromiseInvokeButton>
      {logs && (
        <Dialog open onClose={() => setLogs(null)} fullWidth maxWidth="lg">
          <DialogTitle>Logs of swap {swapId}</DialogTitle>
          <DialogContent>
            <CliLogsBox
              minHeight="min(20rem, 70vh)"
              logs={logs}
              label="Logs relevant to the swap"
            />
          </DialogContent>
          <DialogActions>
            <Button
              onClick={() => setLogs(null)}
              variant="contained"
              color="primary"
            >
              Close
            </Button>
          </DialogActions>
        </Dialog>
      )}
    </>
  );
}
