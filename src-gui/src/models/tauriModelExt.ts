import {
  ExpiredTimelocks,
  GetSwapInfoResponse,
  TauriSwapProgressEvent,
} from "./tauriModel";

export type TauriSwapProgressEventContent<
  T extends TauriSwapProgressEvent["type"],
> = Extract<TauriSwapProgressEvent, { type: T }>["content"];

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
  BtcRefunded = "btc is refunded",
  XmrRedeemed = "xmr is redeemed",
  BtcPunished = "btc is punished",
  SafelyAborted = "safely aborted",
}

// TODO: This is a temporary solution until we have a typeshare definition for BobStateName
export type GetSwapInfoResponseExt = GetSwapInfoResponse & {
  state_name: BobStateName;
};

export type TimelockNone = Extract<ExpiredTimelocks, { type: "None" }>;
export type TimelockCancel = Extract<ExpiredTimelocks, { type: "Cancel" }>;
export type TimelockPunish = Extract<ExpiredTimelocks, { type: "Punish" }>;

export type BobStateNameRunningSwap = Exclude<
  BobStateName,
  | BobStateName.Started
  | BobStateName.SwapSetupCompleted
  | BobStateName.BtcRefunded
  | BobStateName.BtcPunished
  | BobStateName.SafelyAborted
  | BobStateName.XmrRedeemed
>;

export type GetSwapInfoResponseExtRunningSwap = GetSwapInfoResponseExt & {
  stateName: BobStateNameRunningSwap;
};

export function isBobStateNameRunningSwap(
  state: BobStateName,
): state is BobStateNameRunningSwap {
  return ![
    BobStateName.Started,
    BobStateName.SwapSetupCompleted,
    BobStateName.BtcRefunded,
    BobStateName.BtcPunished,
    BobStateName.SafelyAborted,
    BobStateName.XmrRedeemed,
  ].includes(state);
}

export type BobStateNameCompletedSwap =
  | BobStateName.XmrRedeemed
  | BobStateName.BtcRefunded
  | BobStateName.BtcPunished
  | BobStateName.SafelyAborted;

export function isBobStateNameCompletedSwap(
  state: BobStateName,
): state is BobStateNameCompletedSwap {
  return [
    BobStateName.XmrRedeemed,
    BobStateName.BtcRefunded,
    BobStateName.BtcPunished,
    BobStateName.SafelyAborted,
  ].includes(state);
}

export type BobStateNamePossiblyCancellableSwap =
  | BobStateName.BtcLocked
  | BobStateName.XmrLockProofReceived
  | BobStateName.XmrLocked
  | BobStateName.EncSigSent
  | BobStateName.CancelTimelockExpired;

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
export function isBobStateNamePossiblyCancellableSwap(
  state: BobStateName,
): state is BobStateNamePossiblyCancellableSwap {
  return [
    BobStateName.BtcLocked,
    BobStateName.XmrLockProofReceived,
    BobStateName.XmrLocked,
    BobStateName.EncSigSent,
    BobStateName.CancelTimelockExpired,
  ].includes(state);
}

export type BobStateNamePossiblyRefundableSwap =
  | BobStateName.BtcLocked
  | BobStateName.XmrLockProofReceived
  | BobStateName.XmrLocked
  | BobStateName.EncSigSent
  | BobStateName.CancelTimelockExpired
  | BobStateName.BtcCancelled;

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
