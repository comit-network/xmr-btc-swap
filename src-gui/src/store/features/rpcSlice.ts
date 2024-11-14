import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { ExtendedProviderStatus, ProviderStatus } from "models/apiModel";
import {
  TauriLogEvent,
  GetSwapInfoResponse,
  TauriContextStatusEvent,
  TauriTimelockChangeEvent,
  BackgroundRefundState,
} from "models/tauriModel";
import { MoneroRecoveryResponse } from "../../models/rpcModel";
import { GetSwapInfoResponseExt } from "models/tauriModelExt";
import { getLogsAndStringsFromRawFileString } from "utils/parseUtils";
import { CliLog } from "models/cliModel";
import logger from "utils/logger";

interface State {
  balance: number | null;
  withdrawTxId: string | null;
  rendezvous_discovered_sellers: (ExtendedProviderStatus | ProviderStatus)[];
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
      slice.logs = slice.logs.concat(logs);
    },
    contextStatusEventReceived(
      slice,
      action: PayloadAction<TauriContextStatusEvent>,
    ) {
      slice.status = action.payload;
    },
    timelockChangeEventReceived(
      slice,
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
    rpcSetRendezvousDiscoveredProviders(
      slice,
      action: PayloadAction<(ExtendedProviderStatus | ProviderStatus)[]>,
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
  },
});

export const {
  contextStatusEventReceived,
  receivedCliLog,
  rpcSetBalance,
  rpcSetWithdrawTxId,
  rpcResetWithdrawTxId,
  rpcSetRendezvousDiscoveredProviders,
  rpcSetSwapInfo,
  rpcSetMoneroRecoveryKeys,
  rpcResetMoneroRecoveryKeys,
  rpcSetBackgroundRefundState,
  timelockChangeEventReceived
} = rpcSlice.actions;

export default rpcSlice.reducer;
