import { Box, DialogContentText } from "@mui/material";
import {
  useActiveSwapInfo,
  useActiveSwapLogs,
  useAppSelector,
} from "store/hooks";
import JsonTreeView from "../../../other/JSONViewTree";
import CliLogsBox from "../../../other/RenderedCliLog";

export default function DebugPage() {
  const torStdOut = useAppSelector((s) => s.tor.stdOut);
  const logs = useActiveSwapLogs();
  const guiState = useAppSelector((s) => s);
  const cliState = useActiveSwapInfo();

  return (
    <Box>
      <DialogContentText>
        <Box
          style={{
            display: "flex",
            flexDirection: "column",
            gap: "8px",
          }}
        >
          <CliLogsBox
            logs={logs}
            label="Logs relevant to the swap (only current session)"
          />
          <JsonTreeView
            data={guiState}
            label="Internal GUI State (inferred from Logs)"
          />
          <JsonTreeView
            data={cliState}
            label="Swap Daemon State (exposed via API)"
          />
          <CliLogsBox
            label="Tor Daemon Logs"
            logs={(torStdOut || "").split("\n")}
          />
        </Box>
      </DialogContentText>
    </Box>
  );
}
