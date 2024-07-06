export enum SwapSpawnType {
  INIT = 'init',
  RESUME = 'resume',
  CANCEL_REFUND = 'cancel-refund',
}

export type CliLogSpanType = string | 'BitcoinWalletSubscription';

export interface CliLog {
  timestamp: string;
  level: 'DEBUG' | 'INFO' | 'WARN' | 'ERROR' | 'TRACE';
  fields: {
    message: string;
    [index: string]: unknown;
  };
  spans?: {
    name: CliLogSpanType;
    [index: string]: unknown;
  }[];
}

export function isCliLog(log: unknown): log is CliLog {
  if (log && typeof log === 'object') {
    return (
      'timestamp' in (log as CliLog) &&
      'level' in (log as CliLog) &&
      'fields' in (log as CliLog) &&
      typeof (log as CliLog).fields?.message === 'string'
    );
  }
  return false;
}

export interface CliLogStartedRpcServer extends CliLog {
  fields: {
    message: 'Started RPC server';
    addr: string;
  };
}

export function isCliLogStartedRpcServer(
  log: CliLog,
): log is CliLogStartedRpcServer {
  return log.fields.message === 'Started RPC server';
}

export interface CliLogReleasingSwapLockLog extends CliLog {
  fields: {
    message: 'Releasing swap lock';
    swap_id: string;
  };
}

export function isCliLogReleasingSwapLockLog(
  log: CliLog,
): log is CliLogReleasingSwapLockLog {
  return log.fields.message === 'Releasing swap lock';
}

export interface CliLogApiCallError extends CliLog {
  fields: {
    message: 'API call resulted in an error';
    err: string;
  };
}

export function isCliLogApiCallError(log: CliLog): log is CliLogApiCallError {
  return log.fields.message === 'API call resulted in an error';
}

export interface CliLogAcquiringSwapLockLog extends CliLog {
  fields: {
    message: 'Acquiring swap lock';
    swap_id: string;
  };
}

export function isCliLogAcquiringSwapLockLog(
  log: CliLog,
): log is CliLogAcquiringSwapLockLog {
  return log.fields.message === 'Acquiring swap lock';
}

export interface CliLogReceivedQuote extends CliLog {
  fields: {
    message: 'Received quote';
    price: string;
    minimum_amount: string;
    maximum_amount: string;
  };
}

export function isCliLogReceivedQuote(log: CliLog): log is CliLogReceivedQuote {
  return log.fields.message === 'Received quote';
}

export interface CliLogWaitingForBtcDeposit extends CliLog {
  fields: {
    message: 'Waiting for Bitcoin deposit';
    deposit_address: string;
    min_deposit_until_swap_will_start: string;
    max_deposit_until_maximum_amount_is_reached: string;
    max_giveable: string;
    minimum_amount: string;
    maximum_amount: string;
    min_bitcoin_lock_tx_fee: string;
    price: string;
  };
}

export function isCliLogWaitingForBtcDeposit(
  log: CliLog,
): log is CliLogWaitingForBtcDeposit {
  return log.fields.message === 'Waiting for Bitcoin deposit';
}

export interface CliLogReceivedBtc extends CliLog {
  fields: {
    message: 'Received Bitcoin';
    max_giveable: string;
    new_balance: string;
  };
}

export function isCliLogReceivedBtc(log: CliLog): log is CliLogReceivedBtc {
  return log.fields.message === 'Received Bitcoin';
}

export interface CliLogDeterminedSwapAmount extends CliLog {
  fields: {
    message: 'Determined swap amount';
    amount: string;
    fees: string;
  };
}

export function isCliLogDeterminedSwapAmount(
  log: CliLog,
): log is CliLogDeterminedSwapAmount {
  return log.fields.message === 'Determined swap amount';
}

export interface CliLogStartedSwap extends CliLog {
  fields: {
    message: 'Starting new swap';
    swap_id: string;
  };
}

export function isCliLogStartedSwap(log: CliLog): log is CliLogStartedSwap {
  return log.fields.message === 'Starting new swap';
}

export interface CliLogPublishedBtcTx extends CliLog {
  fields: {
    message: 'Published Bitcoin transaction';
    txid: string;
    kind: 'lock' | 'cancel' | 'withdraw' | 'refund';
  };
}

export function isCliLogPublishedBtcTx(
  log: CliLog,
): log is CliLogPublishedBtcTx {
  return log.fields.message === 'Published Bitcoin transaction';
}

export interface CliLogBtcTxFound extends CliLog {
  fields: {
    message: 'Found relevant Bitcoin transaction';
    txid: string;
    status: string;
  };
}

export function isCliLogBtcTxFound(log: CliLog): log is CliLogBtcTxFound {
  return log.fields.message === 'Found relevant Bitcoin transaction';
}

export interface CliLogBtcTxStatusChanged extends CliLog {
  fields: {
    message: 'Bitcoin transaction status changed';
    txid: string;
    new_status: string;
  };
}

export function isCliLogBtcTxStatusChanged(
  log: CliLog,
): log is CliLogBtcTxStatusChanged {
  return log.fields.message === 'Bitcoin transaction status changed';
}

export interface CliLogAliceLockedXmr extends CliLog {
  fields: {
    message: 'Alice locked Monero';
    txid: string;
  };
}

export function isCliLogAliceLockedXmr(
  log: CliLog,
): log is CliLogAliceLockedXmr {
  return log.fields.message === 'Alice locked Monero';
}

export interface CliLogReceivedXmrLockTxConfirmation extends CliLog {
  fields: {
    message: 'Received new confirmation for Monero lock tx';
    txid: string;
    seen_confirmations: string;
    needed_confirmations: string;
  };
}

export function isCliLogReceivedXmrLockTxConfirmation(
  log: CliLog,
): log is CliLogReceivedXmrLockTxConfirmation {
  return log.fields.message === 'Received new confirmation for Monero lock tx';
}

export interface CliLogAdvancingState extends CliLog {
  fields: {
    message: 'Advancing state';
    state:
      | 'quote has been requested'
      | 'execution setup done'
      | 'btc is locked'
      | 'XMR lock transaction transfer proof received'
      | 'xmr is locked'
      | 'encrypted signature is sent'
      | 'btc is redeemed'
      | 'cancel timelock is expired'
      | 'btc is cancelled'
      | 'btc is refunded'
      | 'xmr is redeemed'
      | 'btc is punished'
      | 'safely aborted';
  };
}

export function isCliLogAdvancingState(
  log: CliLog,
): log is CliLogAdvancingState {
  return log.fields.message === 'Advancing state';
}

export interface CliLogRedeemedXmr extends CliLog {
  fields: {
    message: 'Successfully transferred XMR to wallet';
    monero_receive_address: string;
    txid: string;
  };
}

export function isCliLogRedeemedXmr(log: CliLog): log is CliLogRedeemedXmr {
  return log.fields.message === 'Successfully transferred XMR to wallet';
}

export interface YouHaveBeenPunishedCliLog extends CliLog {
  fields: {
    message: 'You have been punished for not refunding in time';
  };
}

export function isYouHaveBeenPunishedCliLog(
  log: CliLog,
): log is YouHaveBeenPunishedCliLog {
  return (
    log.fields.message === 'You have been punished for not refunding in time'
  );
}

function getCliLogSpanAttribute<T>(log: CliLog, key: string): T | null {
  const span = log.spans?.find((s) => s[key]);
  if (!span) {
    return null;
  }
  return span[key] as T;
}

export function getCliLogSpanSwapId(log: CliLog): string | null {
  return getCliLogSpanAttribute<string>(log, 'swap_id');
}

export function getCliLogSpanLogReferenceId(log: CliLog): string | null {
  return (
    getCliLogSpanAttribute<string>(log, 'log_reference_id')?.replace(
      /"/g,
      '',
    ) || null
  );
}

export function hasCliLogOneOfMultipleSpans(
  log: CliLog,
  spanNames: string[],
): boolean {
  return log.spans?.some((s) => spanNames.includes(s.name)) ?? false;
}

export interface CliLogStartedSyncingMoneroWallet extends CliLog {
  fields: {
    message: 'Syncing Monero wallet';
    current_sync_height?: boolean;
  };
}

export function isCliLogStartedSyncingMoneroWallet(
  log: CliLog,
): log is CliLogStartedSyncingMoneroWallet {
  return log.fields.message === 'Syncing Monero wallet';
}

export interface CliLogFinishedSyncingMoneroWallet extends CliLog {
  fields: {
    message: 'Synced Monero wallet';
  };
}

export interface CliLogFailedToSyncMoneroWallet extends CliLog {
  fields: {
    message: 'Failed to sync Monero wallet';
    error: string;
  };
}

export function isCliLogFailedToSyncMoneroWallet(
  log: CliLog,
): log is CliLogFailedToSyncMoneroWallet {
  return log.fields.message === 'Failed to sync Monero wallet';
}

export function isCliLogFinishedSyncingMoneroWallet(
  log: CliLog,
): log is CliLogFinishedSyncingMoneroWallet {
  return log.fields.message === 'Monero wallet synced';
}

export interface CliLogDownloadingMoneroWalletRpc extends CliLog {
  fields: {
    message: 'Downloading monero-wallet-rpc';
    progress: string;
    size: string;
    download_url: string;
  };
}

export function isCliLogDownloadingMoneroWalletRpc(
  log: CliLog,
): log is CliLogDownloadingMoneroWalletRpc {
  return log.fields.message === 'Downloading monero-wallet-rpc';
}

export interface CliLogStartedSyncingMoneroWallet extends CliLog {
  fields: {
    message: 'Syncing Monero wallet';
    current_sync_height?: boolean;
  };
}

export interface CliLogDownloadingMoneroWalletRpc extends CliLog {
  fields: {
    message: 'Downloading monero-wallet-rpc';
    progress: string;
    size: string;
    download_url: string;
  };
}

export interface CliLogGotNotificationForNewBlock extends CliLog {
  fields: {
    message: 'Got notification for new block';
    block_height: string;
  };
}

export function isCliLogGotNotificationForNewBlock(
  log: CliLog,
): log is CliLogGotNotificationForNewBlock {
  return log.fields.message === 'Got notification for new block';
}

export interface CliLogAttemptingToCooperativelyRedeemXmr extends CliLog {
  fields: {
    message: 'Attempting to cooperatively redeem XMR after being punished';
  };
}

export function isCliLogAttemptingToCooperativelyRedeemXmr(
  log: CliLog,
): log is CliLogAttemptingToCooperativelyRedeemXmr {
  return log.fields.message === 'Attempting to cooperatively redeem XMR after being punished';
}

export interface CliLogAliceHasAcceptedOurRequestToCooperativelyRedeemTheXmr extends CliLog {
  fields: {
    message: 'Alice has accepted our request to cooperatively redeem the XMR';
  };
}

export function isCliLogAliceHasAcceptedOurRequestToCooperativelyRedeemTheXmr(
  log: CliLog,
): log is CliLogAliceHasAcceptedOurRequestToCooperativelyRedeemTheXmr {
  return log.fields.message === 'Alice has accepted our request to cooperatively redeem the XMR';
}