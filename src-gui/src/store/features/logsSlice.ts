import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { TauriLogEvent } from "models/tauriModel";
import { parseLogsFromString } from "utils/parseUtils";
import { CliLog } from "models/cliModel";

interface LogsState {
  logs: (CliLog | string)[];
}

export interface LogsSlice {
  state: LogsState;
}

const initialState: LogsSlice = {
  state: {
    logs: [],
  },
};

export const logsSlice = createSlice({
  name: "logs",
  initialState,
  reducers: {
    receivedCliLog(slice, action: PayloadAction<TauriLogEvent>) {
      const buffer = action.payload.buffer;
      const logs = parseLogsFromString(buffer);
      const logsWithoutExisting = logs.filter(
        (log) => !slice.state.logs.includes(log),
      );
      slice.state.logs = slice.state.logs.concat(logsWithoutExisting);
    },
  },
});

export const { receivedCliLog } = logsSlice.actions;

export default logsSlice.reducer;
