import { invoke as invokeUnsafe } from "@tauri-apps/api/core";
import {
  BalanceArgs,
  BalanceResponse,
  BuyXmrArgs,
  BuyXmrResponse,
  GetLogsArgs,
  GetLogsResponse,
  GetSwapInfoResponse,
  ListSellersArgs,
  MoneroRecoveryArgs,
  ResumeSwapArgs,
  ResumeSwapResponse,
  SuspendCurrentSwapResponse,
  WithdrawBtcArgs,
  WithdrawBtcResponse,
  GetSwapInfoArgs,
  ExportBitcoinWalletResponse,
  CheckMoneroNodeArgs,
  CheckMoneroNodeResponse,
  TauriSettings,
  CheckElectrumNodeArgs,
  CheckElectrumNodeResponse,
  GetMoneroAddressesResponse,
  GetDataDirArgs,
  ResolveApprovalArgs,
  ResolveApprovalResponse,
  RedactArgs,
  RedactResponse,
} from "models/tauriModel";
import { rpcSetBalance, rpcSetSwapInfo } from "store/features/rpcSlice";
import { store } from "./store/storeRenderer";
import { Maker } from "models/apiModel";
import { providerToConcatenatedMultiAddr } from "utils/multiAddrUtils";
import { MoneroRecoveryResponse } from "models/rpcModel";
import { ListSellersResponse } from "../models/tauriModel";
import logger from "utils/logger";
import { getNetwork, isTestnet } from "store/config";
import { Blockchain, Network } from "store/features/settingsSlice";
import { setStatus } from "store/features/nodesSlice";
import { discoveredMakersByRendezvous } from "store/features/makersSlice";
import { CliLog } from "models/cliModel";
import { logsToRawString, parseLogsFromString } from "utils/parseUtils";

export const PRESET_RENDEZVOUS_POINTS = [
  "/dnsaddr/xxmr.cheap/p2p/12D3KooWMk3QyPS8D1d1vpHZoY7y2MnXdPE5yV6iyPvyuj4zcdxT",
];

export async function fetchSellersAtPresetRendezvousPoints() {
  await Promise.all(
    PRESET_RENDEZVOUS_POINTS.map(async (rendezvousPoint) => {
      const response = await listSellersAtRendezvousPoint([rendezvousPoint]);
      store.dispatch(discoveredMakersByRendezvous(response.sellers));

      logger.info(
        `Discovered ${response.sellers.length} sellers at rendezvous point ${rendezvousPoint} during startup fetch`,
      );
    }),
  );
}

async function invoke<ARGS, RESPONSE>(
  command: string,
  args: ARGS,
): Promise<RESPONSE> {
  return invokeUnsafe(command, {
    args: args as Record<string, unknown>,
  }) as Promise<RESPONSE>;
}

async function invokeNoArgs<RESPONSE>(command: string): Promise<RESPONSE> {
  return invokeUnsafe(command) as Promise<RESPONSE>;
}

export async function checkBitcoinBalance() {
  // If we are already syncing, don't start a new sync
  if (
    Object.values(store.getState().rpc?.state.background ?? {}).some(
      (progress) =>
        progress.componentName === "SyncingBitcoinWallet" &&
        progress.progress.type === "Pending",
    )
  ) {
    console.log(
      "checkBitcoinBalance() was called but we are already syncing Bitcoin, skipping",
    );
    return;
  }

  const response = await invoke<BalanceArgs, BalanceResponse>("get_balance", {
    force_refresh: true,
  });

  store.dispatch(rpcSetBalance(response.balance));
}

export async function cheapCheckBitcoinBalance() {
  const response = await invoke<BalanceArgs, BalanceResponse>("get_balance", {
    force_refresh: false,
  });

  store.dispatch(rpcSetBalance(response.balance));
}

export async function getAllSwapInfos() {
  const response =
    await invokeNoArgs<GetSwapInfoResponse[]>("get_swap_infos_all");

  response.forEach((swapInfo) => {
    store.dispatch(rpcSetSwapInfo(swapInfo));
  });
}

export async function getSwapInfo(swapId: string) {
  const response = await invoke<GetSwapInfoArgs, GetSwapInfoResponse>(
    "get_swap_info",
    {
      swap_id: swapId,
    },
  );

  store.dispatch(rpcSetSwapInfo(response));
}

export async function withdrawBtc(address: string): Promise<string> {
  const response = await invoke<WithdrawBtcArgs, WithdrawBtcResponse>(
    "withdraw_btc",
    {
      address,
      amount: null,
    },
  );

  // We check the balance, this is cheap and does not sync the wallet
  // but instead uses our local cached balance
  await cheapCheckBitcoinBalance();

  return response.txid;
}

export async function buyXmr(
  seller: Maker,
  bitcoin_change_address: string | null,
  monero_receive_address: string,
) {
  await invoke<BuyXmrArgs, BuyXmrResponse>(
    "buy_xmr",
    bitcoin_change_address == null
      ? {
          seller: providerToConcatenatedMultiAddr(seller),
          monero_receive_address,
        }
      : {
          seller: providerToConcatenatedMultiAddr(seller),
          monero_receive_address,
          bitcoin_change_address,
        },
  );
}

export async function resumeSwap(swapId: string) {
  await invoke<ResumeSwapArgs, ResumeSwapResponse>("resume_swap", {
    swap_id: swapId,
  });
}

export async function suspendCurrentSwap() {
  await invokeNoArgs<SuspendCurrentSwapResponse>("suspend_current_swap");
}

export async function getMoneroRecoveryKeys(
  swapId: string,
): Promise<MoneroRecoveryResponse> {
  return await invoke<MoneroRecoveryArgs, MoneroRecoveryResponse>(
    "monero_recovery",
    {
      swap_id: swapId,
    },
  );
}

export async function checkContextAvailability(): Promise<boolean> {
  const available = await invokeNoArgs<boolean>("is_context_available");
  return available;
}

export async function getLogsOfSwap(
  swapId: string,
  redact: boolean,
): Promise<GetLogsResponse> {
  return await invoke<GetLogsArgs, GetLogsResponse>("get_logs", {
    swap_id: swapId,
    redact,
  });
}

/// Call the rust backend to redact logs.
export async function redactLogs(
  logs: (string | CliLog)[],
): Promise<(string | CliLog)[]> {
  const response = await invoke<RedactArgs, RedactResponse>("redact", {
    text: logsToRawString(logs),
  });

  return parseLogsFromString(response.text);
}

export async function listSellersAtRendezvousPoint(
  rendezvousPointAddresses: string[],
): Promise<ListSellersResponse> {
  return await invoke<ListSellersArgs, ListSellersResponse>("list_sellers", {
    rendezvous_points: rendezvousPointAddresses,
  });
}

export async function initializeContext() {
  const network = getNetwork();
  const testnet = isTestnet();
  const useTor = store.getState().settings.enableTor;

  // Get all Bitcoin nodes without checking availability
  // The backend ElectrumBalancer will handle load balancing and failover
  const bitcoinNodes =
    store.getState().settings.nodes[network][Blockchain.Bitcoin];

  // For Monero nodes, get the configured node URL and pool setting
  const useMoneroRpcPool = store.getState().settings.useMoneroRpcPool;
  const moneroNodes = store.getState().settings.nodes[network][Blockchain.Monero];
  
  // Always pass the first configured monero node URL directly without checking availability
  // The backend will handle whether to use the pool or the custom node
  const moneroNode = moneroNodes.length > 0 ? moneroNodes[0] : null;

  // Initialize Tauri settings
  const tauriSettings: TauriSettings = {
    electrum_rpc_urls: bitcoinNodes,
    monero_node_url: moneroNode,
    use_tor: useTor,
    use_monero_rpc_pool: useMoneroRpcPool,
  };

  logger.info("Initializing context with settings", tauriSettings);

  await invokeUnsafe<void>("initialize_context", {
    settings: tauriSettings,
    testnet,
  });
}

export async function getWalletDescriptor() {
  return await invokeNoArgs<ExportBitcoinWalletResponse>(
    "get_wallet_descriptor",
  );
}

export async function getMoneroNodeStatus(
  node: string,
  network: Network,
): Promise<boolean> {
  const response = await invoke<CheckMoneroNodeArgs, CheckMoneroNodeResponse>(
    "check_monero_node",
    {
      url: node,
      network,
    },
  );

  return response.available;
}

export async function getElectrumNodeStatus(url: string): Promise<boolean> {
  const response = await invoke<
    CheckElectrumNodeArgs,
    CheckElectrumNodeResponse
  >("check_electrum_node", {
    url,
  });

  return response.available;
}

export async function getNodeStatus(
  url: string,
  blockchain: Blockchain,
  network: Network,
): Promise<boolean> {
  switch (blockchain) {
    case Blockchain.Monero:
      return await getMoneroNodeStatus(url, network);
    case Blockchain.Bitcoin:
      return await getElectrumNodeStatus(url);
    default:
      throw new Error(`Unsupported blockchain: ${blockchain}`);
  }
}

async function updateNodeStatus(
  node: string,
  blockchain: Blockchain,
  network: Network,
) {
  const status = await getNodeStatus(node, blockchain, network);

  store.dispatch(setStatus({ node, status, blockchain }));
}

export async function updateAllNodeStatuses() {
  const network = getNetwork();
  const settings = store.getState().settings;

  // Only check Monero nodes if we're using custom nodes (not RPC pool)
  // Skip Bitcoin nodes since we pass all electrum servers to the backend without checking them (ElectrumBalancer handles failover)
  if (!settings.useMoneroRpcPool) {
    await Promise.all(
      settings.nodes[network][Blockchain.Monero].map((node) =>
        updateNodeStatus(node, Blockchain.Monero, network),
      ),
    );
  }
}

export async function getMoneroAddresses(): Promise<GetMoneroAddressesResponse> {
  return await invokeNoArgs<GetMoneroAddressesResponse>("get_monero_addresses");
}

export async function getDataDir(): Promise<string> {
  const testnet = isTestnet();
  return await invoke<GetDataDirArgs, string>("get_data_dir", {
    is_testnet: testnet,
  });
}

export async function resolveApproval(
  requestId: string,
  accept: boolean,
): Promise<void> {
  await invoke<ResolveApprovalArgs, ResolveApprovalResponse>(
    "resolve_approval_request",
    { request_id: requestId, accept },
  );
}

export async function saveLogFiles(
  zipFileName: string,
  content: Record<string, string>,
): Promise<void> {
  await invokeUnsafe<void>("save_txt_files", { zipFileName, content });
}

export async function saveFilesInDialog(files: Record<string, string>) {
  await invokeUnsafe<void>("save_txt_files", {
    files,
  });
}
