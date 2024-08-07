import { createSlice, PayloadAction } from '@reduxjs/toolkit';
import { ExtendedProviderStatus, ProviderStatus } from 'models/apiModel';
import { MoneroWalletRpcUpdateState } from 'models/storeModel';
import {
  GetSwapInfoResponse,
  MoneroRecoveryResponse,
  RpcProcessStateType,
} from '../../models/rpcModel';
import {
  CliLog,
  isCliLog,
  isCliLogDownloadingMoneroWalletRpc,
  isCliLogFailedToSyncMoneroWallet,
  isCliLogFinishedSyncingMoneroWallet,
  isCliLogStartedRpcServer,
  isCliLogStartedSyncingMoneroWallet,
} from '../../models/cliModel';
import { getLogsAndStringsFromRawFileString } from 'utils/parseUtils';

type Process =
  | {
      type: RpcProcessStateType.STARTED;
      logs: (CliLog | string)[];
    }
  | {
      type: RpcProcessStateType.LISTENING_FOR_CONNECTIONS;
      logs: (CliLog | string)[];
      address: string;
    }
  | {
      type: RpcProcessStateType.EXITED;
      logs: (CliLog | string)[];
      exitCode: number | null;
    }
  | {
      type: RpcProcessStateType.NOT_STARTED;
    };

interface State {
  balance: number | null;
  withdrawTxId: string | null;
  rendezvous_discovered_sellers: (ExtendedProviderStatus | ProviderStatus)[];
  swapInfos: {
    [swapId: string]: GetSwapInfoResponse;
  };
  moneroRecovery: {
    swapId: string;
    keys: MoneroRecoveryResponse;
  } | null;
  moneroWallet: {
    isSyncing: boolean;
  };
  moneroWalletRpc: {
    updateState: false | MoneroWalletRpcUpdateState;
  };
}

export interface RPCSlice {
  process: Process;
  state: State;
  busyEndpoints: string[];
}

const initialState: RPCSlice = {
  process: {
    type: RpcProcessStateType.NOT_STARTED,
  },
  state: {
    balance: null,
    withdrawTxId: null,
    rendezvous_discovered_sellers: [],
    swapInfos: {},
    moneroRecovery: null,
    moneroWallet: {
      isSyncing: false,
    },
    moneroWalletRpc: {
      updateState: false,
    },
  },
  busyEndpoints: [],
};

export const rpcSlice = createSlice({
  name: 'rpc',
  initialState,
  reducers: {
    rpcAddLogs(slice, action: PayloadAction<(CliLog | string)[]>) {
      if (
        slice.process.type === RpcProcessStateType.STARTED ||
        slice.process.type === RpcProcessStateType.LISTENING_FOR_CONNECTIONS ||
        slice.process.type === RpcProcessStateType.EXITED
      ) {
        const logs = action.payload;
        slice.process.logs.push(...logs);

        logs.filter(isCliLog).forEach((log) => {
          if (
            isCliLogStartedRpcServer(log) &&
            slice.process.type === RpcProcessStateType.STARTED
          ) {
            slice.process = {
              type: RpcProcessStateType.LISTENING_FOR_CONNECTIONS,
              logs: slice.process.logs,
              address: log.fields.addr,
            };
          } else if (isCliLogDownloadingMoneroWalletRpc(log)) {
            slice.state.moneroWalletRpc.updateState = {
              progress: log.fields.progress,
              downloadUrl: log.fields.download_url,
            };

            if (log.fields.progress === '100%') {
              slice.state.moneroWalletRpc.updateState = false;
            }
          } else if (isCliLogStartedSyncingMoneroWallet(log)) {
            slice.state.moneroWallet.isSyncing = true;
          } else if (isCliLogFinishedSyncingMoneroWallet(log)) {
            slice.state.moneroWallet.isSyncing = false;
          } else if (isCliLogFailedToSyncMoneroWallet(log)) {
            slice.state.moneroWallet.isSyncing = false;
          }
        });
      }
    },
    rpcInitiate(slice) {
      slice.process = {
        type: RpcProcessStateType.STARTED,
        logs: [],
      };
    },
    rpcProcessExited(
      slice,
      action: PayloadAction<{
        exitCode: number | null;
        exitSignal: NodeJS.Signals | null;
      }>,
    ) {
      if (
        slice.process.type === RpcProcessStateType.STARTED ||
        slice.process.type === RpcProcessStateType.LISTENING_FOR_CONNECTIONS
      ) {
        slice.process = {
          type: RpcProcessStateType.EXITED,
          logs: slice.process.logs,
          exitCode: action.payload.exitCode,
        };
        slice.state.moneroWalletRpc = {
          updateState: false,
        };
        slice.state.moneroWallet = {
          isSyncing: false,
        };
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
      slice.state.swapInfos[action.payload.swapId] = action.payload;
    },
    rpcSetEndpointBusy(slice, action: PayloadAction<string>) {
      if (!slice.busyEndpoints.includes(action.payload)) {
        slice.busyEndpoints.push(action.payload);
      }
    },
    rpcSetEndpointFree(slice, action: PayloadAction<string>) {
      const index = slice.busyEndpoints.indexOf(action.payload);
      if (index >= 0) {
        slice.busyEndpoints.splice(index);
      }
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
  },
});

export const {
  rpcProcessExited,
  rpcAddLogs,
  rpcInitiate,
  rpcSetBalance,
  rpcSetWithdrawTxId,
  rpcResetWithdrawTxId,
  rpcSetEndpointBusy,
  rpcSetEndpointFree,
  rpcSetRendezvousDiscoveredProviders,
  rpcSetSwapInfo,
  rpcSetMoneroRecoveryKeys,
  rpcResetMoneroRecoveryKeys,
} = rpcSlice.actions;

export default rpcSlice.reducer;
