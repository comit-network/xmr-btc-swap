import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { ExtendedMakerStatus, MakerStatus } from "models/apiModel";
import {
  TauriLogEvent,
  GetSwapInfoResponse,
  TauriContextStatusEvent,
  TauriTimelockChangeEvent,
  BackgroundRefundState,
  ConfirmationRequestType,
} from "models/tauriModel";
import { MoneroRecoveryResponse } from "../../models/rpcModel";
import { GetSwapInfoResponseExt } from "models/tauriModelExt";
import { getLogsAndStringsFromRawFileString } from "utils/parseUtils";
import { CliLog } from "models/cliModel";
import logger from "utils/logger";

// --- Refactored Confirmation Types ---

// Remove previous definitions if they exist (or ensure they match)
// interface PreBtcLockConfirmationData { ... }
// type ConfirmationRequestData = ...

// This interface represents the actual payload received from the Tauri event `confirmation_request`
// It includes the request_id, timeout, and the flattened generated type
export interface ConfirmationRequestPayload extends ConfirmationRequestType {
  request_id: string;
  timeout_secs: number;
}

// --- End Refactored Confirmation Types ---

interface State {
  balance: number | null;
  withdrawTxId: string | null;
  rendezvous_discovered_sellers: (ExtendedMakerStatus | MakerStatus)[];
  swapInfos: {
    [swapId: string]: GetSwapInfoResponseExt;
  };
  moneroRecovery: {
    swapId: string;
    keys: MoneroRecoveryResponse;
  } | null;
  moneroWalletRpc: {
    // TODO: Reimplement this using Tauri
    updateState: false;
  };
  backgroundRefund: {
    swapId: string;
    state: BackgroundRefundState;
  } | null;
  pendingConfirmations: {
    // Store the full payload, keyed by request_id
    [requestId: string]: ConfirmationRequestPayload;
  };
}

export interface RPCSlice {
  status: TauriContextStatusEvent | null;
  state: State;
  logs: (CliLog | string)[];
}

const initialState: RPCSlice = {
  status: null,
  state: {
    balance: null,
    withdrawTxId: null,
    rendezvous_discovered_sellers: [],
    swapInfos: {},
    moneroRecovery: null,
    moneroWalletRpc: {
      updateState: false,
    },
    backgroundRefund: null,
    pendingConfirmations: {},
  },
  logs: [],
};

export const rpcSlice = createSlice({
  name: "rpc",
  initialState,
  reducers: {
    receivedCliLog(slice, action: PayloadAction<TauriLogEvent>) {
      const buffer = action.payload.buffer;
      const logs = getLogsAndStringsFromRawFileString(buffer);
      const logsWithoutExisting = logs.filter(log => !slice.logs.includes(log));
      slice.logs = slice.logs.concat(logsWithoutExisting);
    },
    contextStatusEventReceived(
      slice,
      action: PayloadAction<TauriContextStatusEvent>,
    ) {
      // If we are already initializing, and we receive a new partial status, we update the existing status
      if (slice.status?.type === "Initializing" && action.payload.type === "Initializing") {
        for (const partialStatus of action.payload.content) {
          // We find the existing status with the same type
          const existingStatus = slice.status.content.find(s => s.componentName === partialStatus.componentName);
          if (existingStatus) {
            // If we find it, we update the content
            existingStatus.progress = partialStatus.progress;
          } else {
            // Otherwise, we add the new partial status
            slice.status.content.push(partialStatus);
          }
        }
      } else {
        // Otherwise, we replace the whole status
        slice.status = action.payload;
      }
    },
    timelockChangeEventReceived(
      slice: RPCSlice,
      action: PayloadAction<TauriTimelockChangeEvent>
    ) {
      if (slice.state.swapInfos[action.payload.swap_id]) {
        slice.state.swapInfos[action.payload.swap_id].timelock = action.payload.timelock;
      } else {
        logger.warn(`Received timelock change event for unknown swap ${action.payload.swap_id}`);
      }
    },
    rpcSetBalance(slice, action: PayloadAction<number>) {
      slice.state.balance = action.payload;
    },
    rpcSetWithdrawTxId(slice, action: PayloadAction<string>) {
      slice.state.withdrawTxId = action.payload;
    },
    rpcSetRendezvousDiscoveredMakers(
      slice,
      action: PayloadAction<(ExtendedMakerStatus | MakerStatus)[]>,
    ) {
      slice.state.rendezvous_discovered_sellers = action.payload;
    },
    rpcResetWithdrawTxId(slice) {
      slice.state.withdrawTxId = null;
    },
    rpcSetSwapInfo(slice, action: PayloadAction<GetSwapInfoResponse>) {
      slice.state.swapInfos[action.payload.swap_id] =
        action.payload as GetSwapInfoResponseExt;
    },
    rpcSetMoneroRecoveryKeys(
      slice,
      action: PayloadAction<[string, MoneroRecoveryResponse]>,
    ) {
      const swapId = action.payload[0];
      const keys = action.payload[1];

      slice.state.moneroRecovery = {
        swapId,
        keys,
      };
    },
    rpcResetMoneroRecoveryKeys(slice) {
      slice.state.moneroRecovery = null;
    },
    rpcSetBackgroundRefundState(slice, action: PayloadAction<{ swap_id: string, state: BackgroundRefundState }>) {
      slice.state.backgroundRefund = {
        swapId: action.payload.swap_id,
        state: action.payload.state,
      };
    },
    confirmationRequested(slice, action: PayloadAction<ConfirmationRequestPayload>) {
      slice.state.pendingConfirmations[action.payload.request_id] = action.payload;
    },
    confirmationResolved(slice, action: PayloadAction<{ requestId: string }>) {
      delete slice.state.pendingConfirmations[action.payload.requestId];
    },
  },
});

export const {
  contextStatusEventReceived,
  receivedCliLog,
  rpcSetBalance,
  rpcSetWithdrawTxId,
  rpcResetWithdrawTxId,
  rpcSetRendezvousDiscoveredMakers,
  rpcSetSwapInfo,
  rpcSetMoneroRecoveryKeys,
  rpcResetMoneroRecoveryKeys,
  rpcSetBackgroundRefundState,
  timelockChangeEventReceived,
  confirmationRequested,
  confirmationResolved,
} = rpcSlice.actions;

export default rpcSlice.reducer;
