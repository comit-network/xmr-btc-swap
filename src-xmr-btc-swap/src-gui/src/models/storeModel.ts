import { CliLog, SwapSpawnType } from './cliModel';
import { Provider } from './apiModel';

export interface SwapSlice {
  state: SwapState | null;
  logs: CliLog[];
  processRunning: boolean;
  provider: Provider | null;
  spawnType: SwapSpawnType | null;
  swapId: string | null;
}

export type MoneroWalletRpcUpdateState = {
  progress: string;
  downloadUrl: string;
};

export interface SwapState {
  type: SwapStateType;
}

export enum SwapStateType {
  INITIATED = 'initiated',
  RECEIVED_QUOTE = 'received quote',
  WAITING_FOR_BTC_DEPOSIT = 'waiting for btc deposit',
  STARTED = 'started',
  BTC_LOCK_TX_IN_MEMPOOL = 'btc lock tx is in mempool',
  XMR_LOCK_TX_IN_MEMPOOL = 'xmr lock tx is in mempool',
  XMR_LOCKED = 'xmr is locked',
  BTC_REDEEMED = 'btc redeemed',
  XMR_REDEEM_IN_MEMPOOL = 'xmr redeem tx is in mempool',
  PROCESS_EXITED = 'process exited',
  BTC_CANCELLED = 'btc cancelled',
  BTC_REFUNDED = 'btc refunded',
  BTC_PUNISHED = 'btc punished',
  ATTEMPTING_COOPERATIVE_REDEEM = 'attempting cooperative redeem',
  COOPERATIVE_REDEEM_REJECTED = 'cooperative redeem rejected',
}

export function isSwapState(state?: SwapState | null): state is SwapState {
  return state?.type != null;
}

export interface SwapStateInitiated extends SwapState {
  type: SwapStateType.INITIATED;
}

export function isSwapStateInitiated(
  state?: SwapState | null,
): state is SwapStateInitiated {
  return state?.type === SwapStateType.INITIATED;
}

export interface SwapStateReceivedQuote extends SwapState {
  type: SwapStateType.RECEIVED_QUOTE;
  price: number;
  minimumSwapAmount: number;
  maximumSwapAmount: number;
}

export function isSwapStateReceivedQuote(
  state?: SwapState | null,
): state is SwapStateReceivedQuote {
  return state?.type === SwapStateType.RECEIVED_QUOTE;
}

export interface SwapStateWaitingForBtcDeposit extends SwapState {
  type: SwapStateType.WAITING_FOR_BTC_DEPOSIT;
  depositAddress: string;
  maxGiveable: number;
  minimumAmount: number;
  maximumAmount: number;
  minDeposit: number;
  maxDeposit: number;
  minBitcoinLockTxFee: number;
  price: number | null;
}

export function isSwapStateWaitingForBtcDeposit(
  state?: SwapState | null,
): state is SwapStateWaitingForBtcDeposit {
  return state?.type === SwapStateType.WAITING_FOR_BTC_DEPOSIT;
}

export interface SwapStateStarted extends SwapState {
  type: SwapStateType.STARTED;
  txLockDetails: {
    amount: number;
    fees: number;
  } | null;
}

export function isSwapStateStarted(
  state?: SwapState | null,
): state is SwapStateStarted {
  return state?.type === SwapStateType.STARTED;
}

export interface SwapStateBtcLockInMempool extends SwapState {
  type: SwapStateType.BTC_LOCK_TX_IN_MEMPOOL;
  bobBtcLockTxId: string;
  bobBtcLockTxConfirmations: number;
}

export function isSwapStateBtcLockInMempool(
  state?: SwapState | null,
): state is SwapStateBtcLockInMempool {
  return state?.type === SwapStateType.BTC_LOCK_TX_IN_MEMPOOL;
}

export interface SwapStateXmrLockInMempool extends SwapState {
  type: SwapStateType.XMR_LOCK_TX_IN_MEMPOOL;
  aliceXmrLockTxId: string;
  aliceXmrLockTxConfirmations: number;
}

export function isSwapStateXmrLockInMempool(
  state?: SwapState | null,
): state is SwapStateXmrLockInMempool {
  return state?.type === SwapStateType.XMR_LOCK_TX_IN_MEMPOOL;
}

export interface SwapStateXmrLocked extends SwapState {
  type: SwapStateType.XMR_LOCKED;
}

export function isSwapStateXmrLocked(
  state?: SwapState | null,
): state is SwapStateXmrLocked {
  return state?.type === SwapStateType.XMR_LOCKED;
}

export interface SwapStateBtcRedemeed extends SwapState {
  type: SwapStateType.BTC_REDEEMED;
}

export function isSwapStateBtcRedemeed(
  state?: SwapState | null,
): state is SwapStateBtcRedemeed {
  return state?.type === SwapStateType.BTC_REDEEMED;
}

export interface SwapStateAttemptingCooperativeRedeeem extends SwapState {
  type: SwapStateType.ATTEMPTING_COOPERATIVE_REDEEM;
}

export function isSwapStateAttemptingCooperativeRedeeem(
  state?: SwapState | null,
): state is SwapStateAttemptingCooperativeRedeeem {
  return state?.type === SwapStateType.ATTEMPTING_COOPERATIVE_REDEEM;
}

export interface SwapStateCooperativeRedeemRejected extends SwapState {
  type: SwapStateType.COOPERATIVE_REDEEM_REJECTED;
  reason: string;
}

export function isSwapStateCooperativeRedeemRejected(
  state?: SwapState | null,
): state is SwapStateCooperativeRedeemRejected {
  return state?.type === SwapStateType.COOPERATIVE_REDEEM_REJECTED;
}

export interface SwapStateXmrRedeemInMempool extends SwapState {
  type: SwapStateType.XMR_REDEEM_IN_MEMPOOL;
  bobXmrRedeemTxId: string;
  bobXmrRedeemAddress: string;
}

export function isSwapStateXmrRedeemInMempool(
  state?: SwapState | null,
): state is SwapStateXmrRedeemInMempool {
  return state?.type === SwapStateType.XMR_REDEEM_IN_MEMPOOL;
}

export interface SwapStateBtcCancelled extends SwapState {
  type: SwapStateType.BTC_CANCELLED;
  btcCancelTxId: string;
}

export function isSwapStateBtcCancelled(
  state?: SwapState | null,
): state is SwapStateBtcCancelled {
  return state?.type === SwapStateType.BTC_CANCELLED;
}

export interface SwapStateBtcRefunded extends SwapState {
  type: SwapStateType.BTC_REFUNDED;
  bobBtcRefundTxId: string;
}

export function isSwapStateBtcRefunded(
  state?: SwapState | null,
): state is SwapStateBtcRefunded {
  return state?.type === SwapStateType.BTC_REFUNDED;
}

export interface SwapStateBtcPunished extends SwapState {
  type: SwapStateType.BTC_PUNISHED;
}

export function isSwapStateBtcPunished(
  state?: SwapState | null,
): state is SwapStateBtcPunished {
  return state?.type === SwapStateType.BTC_PUNISHED;
}

export interface SwapStateProcessExited extends SwapState {
  type: SwapStateType.PROCESS_EXITED;
  prevState: SwapState | null;
  rpcError: string | null;
}

export function isSwapStateProcessExited(
  state?: SwapState | null,
): state is SwapStateProcessExited {
  return state?.type === SwapStateType.PROCESS_EXITED;
}
