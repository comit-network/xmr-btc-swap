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
  CheckSeedArgs,
  CheckSeedResponse,
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
  GetCurrentSwapResponse,
  LabeledMoneroAddress,
  GetMoneroHistoryResponse,
  GetMoneroMainAddressResponse,
  GetMoneroBalanceResponse,
  SendMoneroArgs,
  SendMoneroResponse,
  GetMoneroSyncProgressResponse,
  GetPendingApprovalsResponse,
  RejectApprovalArgs,
  RejectApprovalResponse,
  SetRestoreHeightArgs,
  SetRestoreHeightResponse,
  GetRestoreHeightResponse,
} from "models/tauriModel";
import {
  rpcSetBalance,
  rpcSetSwapInfo,
  approvalRequestsReplaced,
} from "store/features/rpcSlice";
import {
  setMainAddress,
  setBalance,
  setSyncProgress,
  setHistory,
} from "store/features/walletSlice";
import { store } from "./store/storeRenderer";
import { providerToConcatenatedMultiAddr } from "utils/multiAddrUtils";
import { MoneroRecoveryResponse } from "models/rpcModel";
import { ListSellersResponse } from "../models/tauriModel";
import logger from "utils/logger";
import { getNetwork, isTestnet } from "store/config";
import {
  Blockchain,
  DonateToDevelopmentTip,
  Network,
} from "store/features/settingsSlice";
import { setStatus } from "store/features/nodesSlice";
import { discoveredMakersByRendezvous } from "store/features/makersSlice";
import { CliLog } from "models/cliModel";
import { logsToRawString, parseLogsFromString } from "utils/parseUtils";

/// These are the official donation address for the eigenwallet/core project
const DONATION_ADDRESS_MAINNET =
  "49LEH26DJGuCyr8xzRAzWPUryzp7bpccC7Hie1DiwyfJEyUKvMFAethRLybDYrFdU1eHaMkKQpUPebY4WT3cSjEvThmpjPa";
const DONATION_ADDRESS_STAGENET =
  "56E274CJxTyVuuFG651dLURKyneoJ5LsSA5jMq4By9z9GBNYQKG8y5ejTYkcvZxarZW6if14ve8xXav2byK4aRnvNdKyVxp";

/// Signature by binarybaron for the donation address
/// https://github.com/binarybaron/
///
/// Get the key from:
/// - https://github.com/eigenwallet/core/blob/master/utils/gpg_keys/binarybaron.asc
/// - https://unstoppableswap.net/binarybaron.asc
const DONATION_ADDRESS_MAINNET_SIG = `
-----BEGIN PGP SIGNED MESSAGE-----
Hash: SHA512

56E274CJxTyVuuFG651dLURKyneoJ5LsSA5jMq4By9z9GBNYQKG8y5ejTYkcvZxarZW6if14ve8xXav2byK4aRnvNdKyVxp is our donation address (signed by binarybaron)
-----BEGIN PGP SIGNATURE-----

iHUEARYKAB0WIQQ1qETX9LVbxE4YD/GZt10+FHaibgUCaFvzWQAKCRCZt10+FHai
bvC6APoCzCto6RsNYwUr7j1ou3xeVNiwMkUQbE0erKt70pT+tQD/fAvPxHtPyb56
XGFQ0pxL1PKzMd9npBGmGJhC4aTljQ4=
=OUK4
-----END PGP SIGNATURE-----
`;

export const PRESET_RENDEZVOUS_POINTS = [
  "/dns4/discover.unstoppableswap.net/tcp/8888/p2p/12D3KooWA6cnqJpVnreBVnoro8midDL9Lpzmg8oJPoAGi7YYaamE",
  "/dns4/discover2.unstoppableswap.net/tcp/8888/p2p/12D3KooWGRvf7qVQDrNR5nfYD6rKrbgeTi9x8RrbdxbmsPvxL4mw",
  "/dns4/darkness.su/tcp/8888/p2p/12D3KooWFQAgVVS9t9UgL6v1sLprJVM7am5hFK7vy9iBCCoCBYmU",
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
  bitcoin_change_address: string | null,
  monero_receive_address: string,
  donation_percentage: DonateToDevelopmentTip,
) {
  // Get all available makers from the Redux store
  const state = store.getState();
  const allMakers = [
    ...(state.makers.registry.makers || []),
    ...state.makers.rendezvous.makers,
  ];

  // Convert all makers to multiaddr format
  const sellers = allMakers.map((maker) =>
    providerToConcatenatedMultiAddr(maker),
  );

  const address_pool: LabeledMoneroAddress[] = [];
  if (donation_percentage !== false) {
    const donation_address = isTestnet()
      ? DONATION_ADDRESS_STAGENET
      : DONATION_ADDRESS_MAINNET;

    address_pool.push(
      {
        address: monero_receive_address,
        percentage: 1 - donation_percentage,
        label: "Your wallet",
      },
      {
        address: donation_address,
        percentage: donation_percentage,
        label: "Tip to the developers",
      },
    );
  } else {
    address_pool.push({
      address: monero_receive_address,
      percentage: 1,
      label: "Your wallet",
    });
  }

  await invoke<BuyXmrArgs, BuyXmrResponse>("buy_xmr", {
    rendezvous_points: PRESET_RENDEZVOUS_POINTS,
    sellers,
    monero_receive_pool: address_pool,
    bitcoin_change_address,
  });
}

export async function resumeSwap(swapId: string) {
  await invoke<ResumeSwapArgs, ResumeSwapResponse>("resume_swap", {
    swap_id: swapId,
  });
}

export async function suspendCurrentSwap() {
  await invokeNoArgs<SuspendCurrentSwapResponse>("suspend_current_swap");
}

export async function getCurrentSwapId() {
  return await invokeNoArgs<GetCurrentSwapResponse>("get_current_swap");
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

  // For Monero nodes, determine whether to use pool or custom node
  const useMoneroRpcPool = store.getState().settings.useMoneroRpcPool;

  const moneroNodeUrl =
    store.getState().settings.nodes[network][Blockchain.Monero][0] ?? null;

  // Check the state of the Monero node

  const moneroNodeConfig =
    useMoneroRpcPool ||
    moneroNodeUrl == null ||
    !(await getMoneroNodeStatus(moneroNodeUrl, network))
      ? { type: "Pool" as const }
      : {
          type: "SingleNode" as const,
          content: {
            url: moneroNodeUrl,
          },
        };

  // Initialize Tauri settings
  const tauriSettings: TauriSettings = {
    electrum_rpc_urls: bitcoinNodes,
    monero_node_config: moneroNodeConfig,
    use_tor: useTor,
  };

  logger.info("Initializing context with settings", tauriSettings);

  try {
    await invokeUnsafe<void>("initialize_context", {
      settings: tauriSettings,
      testnet,
    });
  } catch (error) {
    throw new Error("Couldn't initialize context: " + error);
  }

  logger.info("Initialized context");
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

export async function getRestoreHeight(): Promise<GetRestoreHeightResponse> {
  return await invokeNoArgs<GetRestoreHeightResponse>("get_restore_height");
}

export async function setMoneroRestoreHeight(
  height: number | Date,
): Promise<SetRestoreHeightResponse> {
  const args: SetRestoreHeightArgs =
    typeof height === "number"
      ? { type: "Height", height: height }
      : {
          type: "Date",
          height: {
            year: height.getFullYear(),
            month: height.getMonth() + 1, // JavaScript months are 0-indexed, but we want 1-indexed
            day: height.getDate(),
          },
        };

  return await invoke<SetRestoreHeightArgs, SetRestoreHeightResponse>(
    "set_monero_restore_height",
    args,
  );
}

export async function getMoneroHistory(): Promise<GetMoneroHistoryResponse> {
  return await invokeNoArgs<GetMoneroHistoryResponse>("get_monero_history");
}

export async function getMoneroMainAddress(): Promise<GetMoneroMainAddressResponse> {
  return await invokeNoArgs<GetMoneroMainAddressResponse>(
    "get_monero_main_address",
  );
}

export async function getMoneroBalance(): Promise<GetMoneroBalanceResponse> {
  return await invokeNoArgs<GetMoneroBalanceResponse>("get_monero_balance");
}

export async function sendMonero(
  args: SendMoneroArgs,
): Promise<SendMoneroResponse> {
  return await invoke<SendMoneroArgs, SendMoneroResponse>("send_monero", args);
}

export async function getMoneroSyncProgress(): Promise<GetMoneroSyncProgressResponse> {
  return await invokeNoArgs<GetMoneroSyncProgressResponse>(
    "get_monero_sync_progress",
  );
}

// Wallet management functions that handle Redux dispatching
export async function initializeMoneroWallet() {
  try {
    const [
      addressResponse,
      balanceResponse,
      syncProgressResponse,
      historyResponse,
    ] = await Promise.all([
      getMoneroMainAddress(),
      getMoneroBalance(),
      getMoneroSyncProgress(),
      getMoneroHistory(),
    ]);

    store.dispatch(setMainAddress(addressResponse.address));
    store.dispatch(setBalance(balanceResponse));
    store.dispatch(setSyncProgress(syncProgressResponse));
    store.dispatch(setHistory(historyResponse));
  } catch (err) {
    console.error("Failed to fetch Monero wallet data:", err);
  }
}

export async function sendMoneroTransaction(
  args: SendMoneroArgs,
): Promise<SendMoneroResponse> {
  try {
    const response = await sendMonero(args);

    // Refresh balance and history after sending - but don't let this block the response
    Promise.all([getMoneroBalance(), getMoneroHistory()])
      .then(([newBalance, newHistory]) => {
        store.dispatch(setBalance(newBalance));
        store.dispatch(setHistory(newHistory));
      })
      .catch((refreshErr) => {
        console.error("Failed to refresh wallet data after send:", refreshErr);
        // Could emit a toast notification here
      });

    return response;
  } catch (err) {
    console.error("Failed to send Monero:", err);
    throw err; // ✅ Re-throw so caller can handle appropriately
  }
}

async function refreshWalletDataAfterTransaction() {
  try {
    const [newBalance, newHistory] = await Promise.all([
      getMoneroBalance(),
      getMoneroHistory(),
    ]);
    store.dispatch(setBalance(newBalance));
    store.dispatch(setHistory(newHistory));
  } catch (err) {
    console.error("Failed to refresh wallet data after transaction:", err);
    // Maybe show a non-blocking notification to user
  }
}

export async function updateMoneroSyncProgress() {
  try {
    const response = await getMoneroSyncProgress();
    store.dispatch(setSyncProgress(response));
  } catch (err) {
    console.error("Failed to fetch sync progress:", err);
  }
}

export async function getDataDir(): Promise<string> {
  const testnet = isTestnet();
  return await invoke<GetDataDirArgs, string>("get_data_dir", {
    is_testnet: testnet,
  });
}

export async function resolveApproval<T>(
  requestId: string,
  accept: T,
): Promise<void> {
  try {
    await invoke<ResolveApprovalArgs, ResolveApprovalResponse>(
      "resolve_approval_request",
      { request_id: requestId, accept: accept as object },
    );
  } finally {
    // Always refresh the approval list
    await refreshApprovals();

    // Refresh the approval list a few miliseconds later to again
    // Just to make sure :)
    setTimeout(() => {
      refreshApprovals();
    }, 200);
  }
}

export async function rejectApproval<T>(
  requestId: string,
  reject: T,
): Promise<void> {
  await invoke<RejectApprovalArgs, RejectApprovalResponse>(
    "reject_approval_request",
    { request_id: requestId },
  );
}

export async function refreshApprovals(): Promise<void> {
  const response = await invokeNoArgs<GetPendingApprovalsResponse>(
    "get_pending_approvals",
  );
  store.dispatch(approvalRequestsReplaced(response.approvals));
}

export async function checkSeed(seed: string): Promise<boolean> {
  const response = await invoke<CheckSeedArgs, CheckSeedResponse>(
    "check_seed",
    {
      seed,
    },
  );
  return response.available;
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
