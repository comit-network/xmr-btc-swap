import { ButtonProps } from '@material-ui/core/Button/Button';
import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
} from '@material-ui/core';
import { useState } from 'react';
import { CliLog } from 'models/cliModel';
import IpcInvokeButton from '../../../IpcInvokeButton';
import CliLogsBox from '../../../other/RenderedCliLog';

export default function SwapLogFileOpenButton({
  swapId,
  ...props
}: { swapId: string } & ButtonProps) {
  const [logs, setLogs] = useState<CliLog[] | null>(null);

  return (
    <>
      <IpcInvokeButton
        ipcArgs={[swapId]}
        ipcChannel="get-swap-logs"
        onSuccess={(data) => {
          setLogs(data as CliLog[]);
        }}
        {...props}
      >
        view log
      </IpcInvokeButton>
      {logs && (
        <Dialog open onClose={() => setLogs(null)} fullWidth maxWidth="lg">
          <DialogTitle>Logs of swap {swapId}</DialogTitle>
          <DialogContent>
            <CliLogsBox logs={logs} label="Logs relevant to the swap" />
          </DialogContent>
          <DialogActions>
            <Button onClick={() => setLogs(null)}>Close</Button>
          </DialogActions>
        </Dialog>
      )}
    </>
  );
}
