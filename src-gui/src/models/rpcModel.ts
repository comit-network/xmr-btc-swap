export enum RpcMethod {
  GET_BTC_BALANCE = "get_bitcoin_balance",
  WITHDRAW_BTC = "withdraw_btc",
  BUY_XMR = "buy_xmr",
  RESUME_SWAP = "resume_swap",
  LIST_SELLERS = "list_sellers",
  CANCEL_REFUND_SWAP = "cancel_refund_swap",
  GET_SWAP_INFO = "get_swap_info",
  SUSPEND_CURRENT_SWAP = "suspend_current_swap",
  GET_HISTORY = "get_history",
  GET_MONERO_RECOVERY_KEYS = "get_monero_recovery_info",
}

export enum RpcProcessStateType {
  STARTED = "starting...",
  LISTENING_FOR_CONNECTIONS = "running",
  EXITED = "exited",
  NOT_STARTED = "not started",
}

export type RawRpcResponseSuccess<T> = {
  jsonrpc: string;
  id: string;
  result: T;
};

export type RawRpcResponseError = {
  jsonrpc: string;
  id: string;
  error: { code: number; message: string };
};

export type RawRpcResponse<T> = RawRpcResponseSuccess<T> | RawRpcResponseError;

export function isSuccessResponse<T>(
  response: RawRpcResponse<T>,
): response is RawRpcResponseSuccess<T> {
  return "result" in response;
}

export function isErrorResponse<T>(
  response: RawRpcResponse<T>,
): response is RawRpcResponseError {
  return "error" in response;
}

export interface RpcSellerStatus {
  status:
    | {
        Online: {
          price: number;
          min_quantity: number;
          max_quantity: number;
        };
      }
    | "Unreachable";
  multiaddr: string;
}

export interface WithdrawBitcoinResponse {
  txid: string;
}

export interface BuyXmrResponse {
  swapId: string;
}

export type SwapTimelockInfoNone = {
  None: {
    blocks_left: number;
  };
};

export type SwapTimelockInfoCancelled = {
  Cancel: {
    blocks_left: number;
  };
};

export type SwapTimelockInfoPunished = "Punish";

export type SwapTimelockInfo =
  | SwapTimelockInfoNone
  | SwapTimelockInfoCancelled
  | SwapTimelockInfoPunished;

export function isSwapTimelockInfoNone(
  info: SwapTimelockInfo,
): info is SwapTimelockInfoNone {
  return typeof info === "object" && "None" in info;
}

export function isSwapTimelockInfoCancelled(
  info: SwapTimelockInfo,
): info is SwapTimelockInfoCancelled {
  return typeof info === "object" && "Cancel" in info;
}

export function isSwapTimelockInfoPunished(
  info: SwapTimelockInfo,
): info is SwapTimelockInfoPunished {
  return info === "Punish";
}

export type SwapSellerInfo = {
  peerId: string;
  addresses: string[];
};

export type MoneroRecoveryResponse = {
  address: string;
  spend_key: string;
  view_key: string;
  restore_height: number;
};
