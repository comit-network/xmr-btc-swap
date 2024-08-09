import { createSlice, PayloadAction } from "@reduxjs/toolkit";

export interface TorSlice {
  exitCode: number | null;
  processRunning: boolean;
  stdOut: string;
  proxyStatus:
    | false
    | {
        proxyHostname: string;
        proxyPort: number;
        bootstrapped: boolean;
      };
}

const initialState: TorSlice = {
  processRunning: false,
  exitCode: null,
  stdOut: "",
  proxyStatus: false,
};

const socksListenerRegex =
  /Opened Socks listener connection.*on (\d+\.\d+\.\d+\.\d+):(\d+)/;
const bootstrapDoneRegex = /Bootstrapped 100% \(done\)/;

export const torSlice = createSlice({
  name: "tor",
  initialState,
  reducers: {
    torAppendStdOut(slice, action: PayloadAction<string>) {
      slice.stdOut += action.payload;

      const logs = slice.stdOut.split("\n");
      logs.forEach((log) => {
        if (socksListenerRegex.test(log)) {
          const match = socksListenerRegex.exec(log);
          if (match) {
            slice.proxyStatus = {
              proxyHostname: match[1],
              proxyPort: Number.parseInt(match[2], 10),
              bootstrapped: slice.proxyStatus
                ? slice.proxyStatus.bootstrapped
                : false,
            };
          }
        } else if (bootstrapDoneRegex.test(log)) {
          if (slice.proxyStatus) {
            slice.proxyStatus.bootstrapped = true;
          }
        }
      });
    },
    torInitiate(slice) {
      slice.processRunning = true;
    },
    torProcessExited(
      slice,
      action: PayloadAction<{
        exitCode: number | null;
        exitSignal: NodeJS.Signals | null;
      }>,
    ) {
      slice.processRunning = false;
      slice.exitCode = action.payload.exitCode;
      slice.proxyStatus = false;
    },
  },
});

export const { torAppendStdOut, torInitiate, torProcessExited } =
  torSlice.actions;

export default torSlice.reducer;
