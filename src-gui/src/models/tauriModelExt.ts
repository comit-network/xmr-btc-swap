import { exhaustiveGuard } from "utils/typescriptUtils";
import {
  ApprovalRequest,
  ExpiredTimelocks,
  GetSwapInfoResponse,
  PendingCompleted,
  TauriBackgroundProgress,
  TauriSwapProgressEvent,
} from "./tauriModel";

export type TauriSwapProgressEventType = TauriSwapProgressEvent["type"];

export type TauriSwapProgressEventContent<
  T extends TauriSwapProgressEventType,
> = Extract<TauriSwapProgressEvent, { type: T }>["content"];

export type TauriSwapProgressEventExt<T extends TauriSwapProgressEventType> =
  Extract<TauriSwapProgressEvent, { type: T }>;

// See /swap/src/protocol/bob/state.rs#L57
// TODO: Replace this with a typeshare definition
export enum BobStateName {
  Started = "quote has been requested",
  SwapSetupCompleted = "execution setup done",
  BtcLocked = "btc is locked",
  XmrLockProofReceived = "XMR lock transaction transfer proof received",
  XmrLocked = "xmr is locked",
  EncSigSent = "encrypted signature is sent",
  BtcRedeemed = "btc is redeemed",
  CancelTimelockExpired = "cancel timelock is expired",
  BtcCancelled = "btc is cancelled",
  BtcRefundPublished = "btc refund is published",
  BtcEarlyRefundPublished = "btc early refund is published",
  BtcRefunded = "btc is refunded",
  BtcEarlyRefunded = "btc is early refunded",
  XmrRedeemed = "xmr is redeemed",
  BtcPunished = "btc is punished",
  SafelyAborted = "safely aborted",
}

export function bobStateNameToHumanReadable(stateName: BobStateName): string {
  switch (stateName) {
    case BobStateName.Started:
      return "Started";
    case BobStateName.SwapSetupCompleted:
      return "Setup completed";
    case BobStateName.BtcLocked:
      return "Bitcoin locked";
    case BobStateName.XmrLockProofReceived:
      return "Monero locked";
    case BobStateName.XmrLocked:
      return "Monero locked and fully confirmed";
    case BobStateName.EncSigSent:
      return "Encrypted signature sent";
    case BobStateName.BtcRedeemed:
      return "Bitcoin redeemed";
    case BobStateName.CancelTimelockExpired:
      return "Cancel timelock expired";
    case BobStateName.BtcCancelled:
      return "Bitcoin cancelled";
    case BobStateName.BtcRefundPublished:
      return "Bitcoin refund published";
    case BobStateName.BtcEarlyRefundPublished:
      return "Bitcoin early refund published";
    case BobStateName.BtcRefunded:
      return "Bitcoin refunded";
    case BobStateName.BtcEarlyRefunded:
      return "Bitcoin early refunded";
    case BobStateName.XmrRedeemed:
      return "Monero redeemed";
    case BobStateName.BtcPunished:
      return "Bitcoin punished";
    case BobStateName.SafelyAborted:
      return "Safely aborted";
    default:
      return exhaustiveGuard(stateName);
  }
}

// TODO: This is a temporary solution until we have a typeshare definition for BobStateName
export type GetSwapInfoResponseExt = GetSwapInfoResponse & {
  state_name: BobStateName;
};

export type TimelockNone = Extract<ExpiredTimelocks, { type: "None" }>;
export type TimelockCancel = Extract<ExpiredTimelocks, { type: "Cancel" }>;
export type TimelockPunish = Extract<ExpiredTimelocks, { type: "Punish" }>;

// This function returns the absolute block number of the timelock relative to the block the tx_lock was included in
export function getAbsoluteBlock(
  timelock: ExpiredTimelocks,
  cancelTimelock: number,
  punishTimelock: number,
): number {
  if (timelock.type === "None") {
    return cancelTimelock - timelock.content.blocks_left;
  }
  if (timelock.type === "Cancel") {
    return cancelTimelock + punishTimelock - timelock.content.blocks_left;
  }
  if (timelock.type === "Punish") {
    return cancelTimelock + punishTimelock;
  }

  // We match all cases
  return exhaustiveGuard(timelock);
}

export type BobStateNameRunningSwap = Exclude<
  BobStateName,
  | BobStateName.Started
  | BobStateName.SwapSetupCompleted
  | BobStateName.BtcRefunded
  | BobStateName.BtcEarlyRefunded
  | BobStateName.BtcPunished
  | BobStateName.SafelyAborted
  | BobStateName.XmrRedeemed
>;

export type GetSwapInfoResponseExtRunningSwap = GetSwapInfoResponseExt & {
  state_name: BobStateNameRunningSwap;
};

export type GetSwapInfoResponseExtWithTimelock = GetSwapInfoResponseExt & {
  timelock: ExpiredTimelocks;
};

export function isBobStateNameRunningSwap(
  state: BobStateName,
): state is BobStateNameRunningSwap {
  return ![
    BobStateName.Started,
    BobStateName.SwapSetupCompleted,
    BobStateName.BtcRefunded,
    BobStateName.BtcEarlyRefunded,
    BobStateName.BtcPunished,
    BobStateName.SafelyAborted,
    BobStateName.XmrRedeemed,
  ].includes(state);
}

export type BobStateNameCompletedSwap =
  | BobStateName.XmrRedeemed
  | BobStateName.BtcRefunded
  | BobStateName.BtcEarlyRefunded
  | BobStateName.BtcPunished
  | BobStateName.SafelyAborted;

export function isBobStateNameCompletedSwap(
  state: BobStateName,
): state is BobStateNameCompletedSwap {
  return [
    BobStateName.XmrRedeemed,
    BobStateName.BtcRefunded,
    BobStateName.BtcEarlyRefunded,
    BobStateName.BtcPunished,
    BobStateName.SafelyAborted,
  ].includes(state);
}

export type BobStateNamePossiblyCancellableSwap =
  | BobStateName.BtcLocked
  | BobStateName.XmrLockProofReceived
  | BobStateName.XmrLocked
  | BobStateName.EncSigSent
  | BobStateName.CancelTimelockExpired
  | BobStateName.BtcRefundPublished
  | BobStateName.BtcEarlyRefundPublished;

/**
Checks if a swap is in a state where it can possibly be cancelled

The following conditions must be met:
 - The bitcoin must be locked
 - The bitcoin must not be redeemed
 - The bitcoin must not be cancelled
 - The bitcoin must not be refunded
 - The bitcoin must not be punished
 - The bitcoin must not be early refunded

See: https://github.com/comit-network/xmr-btc-swap/blob/7023e75bb51ab26dff4c8fcccdc855d781ca4b15/swap/src/cli/cancel.rs#L16-L35
 */
export function isBobStateNamePossiblyCancellableSwap(
  state: BobStateName,
): state is BobStateNamePossiblyCancellableSwap {
  return [
    BobStateName.BtcLocked,
    BobStateName.XmrLockProofReceived,
    BobStateName.XmrLocked,
    BobStateName.EncSigSent,
    BobStateName.CancelTimelockExpired,
    BobStateName.BtcRefundPublished,
    BobStateName.BtcEarlyRefundPublished,
  ].includes(state);
}

export type BobStateNamePossiblyRefundableSwap =
  | BobStateName.BtcLocked
  | BobStateName.XmrLockProofReceived
  | BobStateName.XmrLocked
  | BobStateName.EncSigSent
  | BobStateName.CancelTimelockExpired
  | BobStateName.BtcCancelled
  | BobStateName.BtcRefundPublished
  | BobStateName.BtcEarlyRefundPublished;

/**
Checks if a swap is in a state where it can possibly be refunded (meaning it's not impossible)

The following conditions must be met:
 - The bitcoin must be locked
 - The bitcoin must not be redeemed
 - The bitcoin must not be refunded
 - The bitcoin must not be punished

See: https://github.com/comit-network/xmr-btc-swap/blob/7023e75bb51ab26dff4c8fcccdc855d781ca4b15/swap/src/cli/refund.rs#L16-L34
 */
export function isBobStateNamePossiblyRefundableSwap(
  state: BobStateName,
): state is BobStateNamePossiblyRefundableSwap {
  return [
    BobStateName.BtcLocked,
    BobStateName.XmrLockProofReceived,
    BobStateName.XmrLocked,
    BobStateName.EncSigSent,
    BobStateName.CancelTimelockExpired,
    BobStateName.BtcCancelled,
    BobStateName.BtcRefundPublished,
    BobStateName.BtcEarlyRefundPublished,
  ].includes(state);
}

/**
 * Type guard for GetSwapInfoResponseExt
 * "running" means the swap is in progress and not yet completed
 * If a swap is not "running" it means it is either completed or no Bitcoin have been locked yet
 * @param response
 */
export function isGetSwapInfoResponseRunningSwap(
  response: GetSwapInfoResponseExt,
): response is GetSwapInfoResponseExtRunningSwap {
  return isBobStateNameRunningSwap(response.state_name);
}

/**
 * Type guard for GetSwapInfoResponseExt to ensure timelock is not null
 * @param response The swap info response to check
 * @returns True if the timelock exists, false otherwise
 */
export function isGetSwapInfoResponseWithTimelock(
  response: GetSwapInfoResponseExt,
): response is GetSwapInfoResponseExtWithTimelock {
  return response.timelock !== null;
}

export type PendingApprovalRequest = Extract<
  ApprovalRequest,
  { state: "Pending" }
>;

export type PendingLockBitcoinApprovalRequest = PendingApprovalRequest & {
  content: {
    details: { type: "LockBitcoin" };
  };
};

export function isPendingLockBitcoinApprovalEvent(
  event: ApprovalRequest,
): event is PendingLockBitcoinApprovalRequest {
  // Check if the request is pending
  if (event.state !== "Pending") {
    return false;
  }

  // Check if the request is a LockBitcoin request
  return event.content.details.type === "LockBitcoin";
}

export function isPendingBackgroundProcess(
  process: TauriBackgroundProgress,
): process is TauriBackgroundProgress {
  return process.progress.type === "Pending";
}

export type TauriBitcoinSyncProgress = Extract<
  TauriBackgroundProgress,
  { componentName: "SyncingBitcoinWallet" }
>;

export function isBitcoinSyncProgress(
  progress: TauriBackgroundProgress,
): progress is TauriBitcoinSyncProgress {
  return progress.componentName === "SyncingBitcoinWallet";
}
