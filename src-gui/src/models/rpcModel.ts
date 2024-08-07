import { piconerosToXmr, satsToBtc } from 'utils/conversionUtils';
import { exhaustiveGuard } from 'utils/typescriptUtils';

export enum RpcMethod {
  GET_BTC_BALANCE = 'get_bitcoin_balance',
  WITHDRAW_BTC = 'withdraw_btc',
  BUY_XMR = 'buy_xmr',
  RESUME_SWAP = 'resume_swap',
  LIST_SELLERS = 'list_sellers',
  CANCEL_REFUND_SWAP = 'cancel_refund_swap',
  GET_SWAP_INFO = 'get_swap_info',
  SUSPEND_CURRENT_SWAP = 'suspend_current_swap',
  GET_HISTORY = 'get_history',
  GET_MONERO_RECOVERY_KEYS = 'get_monero_recovery_info',
}

export enum RpcProcessStateType {
  STARTED = 'starting...',
  LISTENING_FOR_CONNECTIONS = 'running',
  EXITED = 'exited',
  NOT_STARTED = 'not started',
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
  return 'result' in response;
}

export function isErrorResponse<T>(
  response: RawRpcResponse<T>,
): response is RawRpcResponseError {
  return 'error' in response;
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
    | 'Unreachable';
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

export type SwapTimelockInfoPunished = 'Punish';

export type SwapTimelockInfo =
  | SwapTimelockInfoNone
  | SwapTimelockInfoCancelled
  | SwapTimelockInfoPunished;

export function isSwapTimelockInfoNone(
  info: SwapTimelockInfo,
): info is SwapTimelockInfoNone {
  return typeof info === 'object' && 'None' in info;
}

export function isSwapTimelockInfoCancelled(
  info: SwapTimelockInfo,
): info is SwapTimelockInfoCancelled {
  return typeof info === 'object' && 'Cancel' in info;
}

export function isSwapTimelockInfoPunished(
  info: SwapTimelockInfo,
): info is SwapTimelockInfoPunished {
  return info === 'Punish';
}

export type SwapSellerInfo = {
  peerId: string;
  addresses: string[];
};

export interface GetSwapInfoResponse {
  swapId: string;
  completed: boolean;
  seller: SwapSellerInfo;
  startDate: string;
  stateName: SwapStateName;
  timelock: null | SwapTimelockInfo;
  txLockId: string;
  txCancelFee: number;
  txRefundFee: number;
  txLockFee: number;
  btcAmount: number;
  xmrAmount: number;
  btcRefundAddress: string;
  cancelTimelock: number;
  punishTimelock: number;
}

export type MoneroRecoveryResponse = {
  address: string;
  spend_key: string;
  view_key: string;
  restore_height: number;
};

export interface BalanceBitcoinResponse {
  balance: number;
}

export interface GetHistoryResponse {
  swaps: [swapId: string, stateName: SwapStateName][];
}

export enum SwapStateName {
  Started = 'quote has been requested',
  SwapSetupCompleted = 'execution setup done',
  BtcLocked = 'btc is locked',
  XmrLockProofReceived = 'XMR lock transaction transfer proof received',
  XmrLocked = 'xmr is locked',
  EncSigSent = 'encrypted signature is sent',
  BtcRedeemed = 'btc is redeemed',
  CancelTimelockExpired = 'cancel timelock is expired',
  BtcCancelled = 'btc is cancelled',
  BtcRefunded = 'btc is refunded',
  XmrRedeemed = 'xmr is redeemed',
  BtcPunished = 'btc is punished',
  SafelyAborted = 'safely aborted',
}

export type SwapStateNameRunningSwap = Exclude<
  SwapStateName,
  | SwapStateName.Started
  | SwapStateName.SwapSetupCompleted
  | SwapStateName.BtcRefunded
  | SwapStateName.BtcPunished
  | SwapStateName.SafelyAborted
  | SwapStateName.XmrRedeemed
>;

export type GetSwapInfoResponseRunningSwap = GetSwapInfoResponse & {
  stateName: SwapStateNameRunningSwap;
};

export function isSwapStateNameRunningSwap(
  state: SwapStateName,
): state is SwapStateNameRunningSwap {
  return ![
    SwapStateName.Started,
    SwapStateName.SwapSetupCompleted,
    SwapStateName.BtcRefunded,
    SwapStateName.BtcPunished,
    SwapStateName.SafelyAborted,
    SwapStateName.XmrRedeemed,
  ].includes(state);
}

export type SwapStateNameCompletedSwap =
  | SwapStateName.XmrRedeemed
  | SwapStateName.BtcRefunded
  | SwapStateName.BtcPunished
  | SwapStateName.SafelyAborted;

export function isSwapStateNameCompletedSwap(
  state: SwapStateName,
): state is SwapStateNameCompletedSwap {
  return [
    SwapStateName.XmrRedeemed,
    SwapStateName.BtcRefunded,
    SwapStateName.BtcPunished,
    SwapStateName.SafelyAborted,
  ].includes(state);
}

export type SwapStateNamePossiblyCancellableSwap =
  | SwapStateName.BtcLocked
  | SwapStateName.XmrLockProofReceived
  | SwapStateName.XmrLocked
  | SwapStateName.EncSigSent
  | SwapStateName.CancelTimelockExpired;

/**
Checks if a swap is in a state where it can possibly be cancelled

The following conditions must be met:
 - The bitcoin must be locked
 - The bitcoin must not be redeemed
 - The bitcoin must not be cancelled
 - The bitcoin must not be refunded
 - The bitcoin must not be punished

See: https://github.com/comit-network/xmr-btc-swap/blob/7023e75bb51ab26dff4c8fcccdc855d781ca4b15/swap/src/cli/cancel.rs#L16-L35
 */
export function isSwapStateNamePossiblyCancellableSwap(
  state: SwapStateName,
): state is SwapStateNamePossiblyCancellableSwap {
  return [
    SwapStateName.BtcLocked,
    SwapStateName.XmrLockProofReceived,
    SwapStateName.XmrLocked,
    SwapStateName.EncSigSent,
    SwapStateName.CancelTimelockExpired,
  ].includes(state);
}

export type SwapStateNamePossiblyRefundableSwap =
  | SwapStateName.BtcLocked
  | SwapStateName.XmrLockProofReceived
  | SwapStateName.XmrLocked
  | SwapStateName.EncSigSent
  | SwapStateName.CancelTimelockExpired
  | SwapStateName.BtcCancelled;

/**
Checks if a swap is in a state where it can possibly be refunded (meaning it's not impossible)

The following conditions must be met:
 - The bitcoin must be locked
 - The bitcoin must not be redeemed
 - The bitcoin must not be refunded
 - The bitcoin must not be punished

See: https://github.com/comit-network/xmr-btc-swap/blob/7023e75bb51ab26dff4c8fcccdc855d781ca4b15/swap/src/cli/refund.rs#L16-L34
 */
export function isSwapStateNamePossiblyRefundableSwap(
  state: SwapStateName,
): state is SwapStateNamePossiblyRefundableSwap {
  return [
    SwapStateName.BtcLocked,
    SwapStateName.XmrLockProofReceived,
    SwapStateName.XmrLocked,
    SwapStateName.EncSigSent,
    SwapStateName.CancelTimelockExpired,
    SwapStateName.BtcCancelled,
  ].includes(state);
}

/**
 * Type guard for GetSwapInfoResponseRunningSwap
 * "running" means the swap is in progress and not yet completed
 * If a swap is not "running" it means it is either completed or no Bitcoin have been locked yet
 * @param response
 */
export function isGetSwapInfoResponseRunningSwap(
  response: GetSwapInfoResponse,
): response is GetSwapInfoResponseRunningSwap {
  return isSwapStateNameRunningSwap(response.stateName);
}

export function isSwapMoneroRecoverable(swapStateName: SwapStateName): boolean {
  return [SwapStateName.BtcRedeemed].includes(swapStateName);
}

// See https://github.com/comit-network/xmr-btc-swap/blob/50ae54141255e03dba3d2b09036b1caa4a63e5a3/swap/src/protocol/bob/state.rs#L55
export function getHumanReadableDbStateType(type: SwapStateName): string {
  switch (type) {
    case SwapStateName.Started:
      return 'Quote has been requested';
    case SwapStateName.SwapSetupCompleted:
      return 'Swap has been initiated';
    case SwapStateName.BtcLocked:
      return 'Bitcoin has been locked';
    case SwapStateName.XmrLockProofReceived:
      return 'Monero lock transaction transfer proof has been received';
    case SwapStateName.XmrLocked:
      return 'Monero has been locked';
    case SwapStateName.EncSigSent:
      return 'Encrypted signature has been sent';
    case SwapStateName.BtcRedeemed:
      return 'Bitcoin has been redeemed';
    case SwapStateName.CancelTimelockExpired:
      return 'Cancel timelock has expired';
    case SwapStateName.BtcCancelled:
      return 'Swap has been cancelled';
    case SwapStateName.BtcRefunded:
      return 'Bitcoin has been refunded';
    case SwapStateName.XmrRedeemed:
      return 'Monero has been redeemed';
    case SwapStateName.BtcPunished:
      return 'Bitcoin has been punished';
    case SwapStateName.SafelyAborted:
      return 'Swap has been safely aborted';
    default:
      return exhaustiveGuard(type);
  }
}

export function getSwapTxFees(swap: GetSwapInfoResponse): number {
  return satsToBtc(swap.txLockFee);
}

export function getSwapBtcAmount(swap: GetSwapInfoResponse): number {
  return satsToBtc(swap.btcAmount);
}

export function getSwapXmrAmount(swap: GetSwapInfoResponse): number {
  return piconerosToXmr(swap.xmrAmount);
}

export function getSwapExchangeRate(swap: GetSwapInfoResponse): number {
  const btcAmount = getSwapBtcAmount(swap);
  const xmrAmount = getSwapXmrAmount(swap);

  return btcAmount / xmrAmount;
}
