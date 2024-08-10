import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { extractAmountFromUnitString } from "utils/parseUtils";
import { Provider } from "models/apiModel";
import {
  isSwapStateBtcLockInMempool,
  isSwapStateProcessExited,
  isSwapStateXmrLockInMempool,
  SwapSlice,
  SwapStateAttemptingCooperativeRedeeem,
  SwapStateBtcCancelled,
  SwapStateBtcLockInMempool,
  SwapStateBtcPunished,
  SwapStateBtcRedemeed,
  SwapStateBtcRefunded,
  SwapStateInitiated,
  SwapStateProcessExited,
  SwapStateReceivedQuote,
  SwapStateStarted,
  SwapStateType,
  SwapStateWaitingForBtcDeposit,
  SwapStateXmrLocked,
  SwapStateXmrLockInMempool,
  SwapStateXmrRedeemInMempool,
} from "../../models/storeModel";
import {
  isCliLogAliceLockedXmr,
  isCliLogBtcTxStatusChanged,
  isCliLogPublishedBtcTx,
  isCliLogReceivedQuote,
  isCliLogReceivedXmrLockTxConfirmation,
  isCliLogRedeemedXmr,
  isCliLogStartedSwap,
  isCliLogWaitingForBtcDeposit,
  CliLog,
  isCliLogAdvancingState,
  SwapSpawnType,
  isCliLogBtcTxFound,
  isCliLogReleasingSwapLockLog,
  isYouHaveBeenPunishedCliLog,
  isCliLogAcquiringSwapLockLog,
  isCliLogApiCallError,
  isCliLogDeterminedSwapAmount,
  isCliLogAttemptingToCooperativelyRedeemXmr,
} from "../../models/cliModel";
import logger from "../../utils/logger";

const initialState: SwapSlice = {
  state: null,
  processRunning: false,
  swapId: null,
  logs: [],
  provider: null,
  spawnType: null,
};

export const swapSlice = createSlice({
  name: "swap",
  initialState,
  reducers: {
    swapAddLog(
      slice,
      action: PayloadAction<{ logs: CliLog[]; isFromRestore: boolean }>,
    ) {
      const { logs } = action.payload;
      slice.logs.push(...logs);

      logs.forEach((log) => {
        if (
          isCliLogAcquiringSwapLockLog(log) &&
          !action.payload.isFromRestore
        ) {
          slice.processRunning = true;
          slice.swapId = log.fields.swap_id;
          // TODO: Maybe we can infer more info here (state) from the log
        } else if (isCliLogReceivedQuote(log)) {
          const price = extractAmountFromUnitString(log.fields.price);
          const minimumSwapAmount = extractAmountFromUnitString(
            log.fields.minimum_amount,
          );
          const maximumSwapAmount = extractAmountFromUnitString(
            log.fields.maximum_amount,
          );

          if (
            price != null &&
            minimumSwapAmount != null &&
            maximumSwapAmount != null
          ) {
            const nextState: SwapStateReceivedQuote = {
              type: SwapStateType.RECEIVED_QUOTE,
              price,
              minimumSwapAmount,
              maximumSwapAmount,
            };

            slice.state = nextState;
          }
        } else if (isCliLogWaitingForBtcDeposit(log)) {
          const maxGiveable = extractAmountFromUnitString(
            log.fields.max_giveable,
          );
          const minDeposit = extractAmountFromUnitString(
            log.fields.min_deposit_until_swap_will_start,
          );
          const maxDeposit = extractAmountFromUnitString(
            log.fields.max_deposit_until_maximum_amount_is_reached,
          );
          const minimumAmount = extractAmountFromUnitString(
            log.fields.minimum_amount,
          );
          const maximumAmount = extractAmountFromUnitString(
            log.fields.maximum_amount,
          );
          const minBitcoinLockTxFee = extractAmountFromUnitString(
            log.fields.min_bitcoin_lock_tx_fee,
          );
          const price = extractAmountFromUnitString(log.fields.price);

          const depositAddress = log.fields.deposit_address;

          if (
            maxGiveable != null &&
            minimumAmount != null &&
            maximumAmount != null &&
            minDeposit != null &&
            maxDeposit != null &&
            minBitcoinLockTxFee != null &&
            price != null
          ) {
            const nextState: SwapStateWaitingForBtcDeposit = {
              type: SwapStateType.WAITING_FOR_BTC_DEPOSIT,
              depositAddress,
              maxGiveable,
              minimumAmount,
              maximumAmount,
              minDeposit,
              maxDeposit,
              price,
              minBitcoinLockTxFee,
            };

            slice.state = nextState;
          }
        } else if (isCliLogDeterminedSwapAmount(log)) {
          const amount = extractAmountFromUnitString(log.fields.amount);
          const fees = extractAmountFromUnitString(log.fields.fees);

          const nextState: SwapStateStarted = {
            type: SwapStateType.STARTED,
            txLockDetails:
              amount != null && fees != null ? { amount, fees } : null,
          };

          slice.state = nextState;
        } else if (isCliLogStartedSwap(log)) {
          if (slice.state?.type !== SwapStateType.STARTED) {
            const nextState: SwapStateStarted = {
              type: SwapStateType.STARTED,
              txLockDetails: null,
            };

            slice.state = nextState;
          }

          slice.swapId = log.fields.swap_id;
        } else if (isCliLogPublishedBtcTx(log)) {
          if (log.fields.kind === "lock") {
            const nextState: SwapStateBtcLockInMempool = {
              type: SwapStateType.BTC_LOCK_TX_IN_MEMPOOL,
              bobBtcLockTxId: log.fields.txid,
              bobBtcLockTxConfirmations: 0,
            };

            slice.state = nextState;
          } else if (log.fields.kind === "cancel") {
            const nextState: SwapStateBtcCancelled = {
              type: SwapStateType.BTC_CANCELLED,
              btcCancelTxId: log.fields.txid,
            };

            slice.state = nextState;
          } else if (log.fields.kind === "refund") {
            const nextState: SwapStateBtcRefunded = {
              type: SwapStateType.BTC_REFUNDED,
              bobBtcRefundTxId: log.fields.txid,
            };

            slice.state = nextState;
          }
        } else if (isCliLogBtcTxStatusChanged(log) || isCliLogBtcTxFound(log)) {
          if (isSwapStateBtcLockInMempool(slice.state)) {
            if (slice.state.bobBtcLockTxId === log.fields.txid) {
              const newStatusText = isCliLogBtcTxStatusChanged(log)
                ? log.fields.new_status
                : log.fields.status;

              if (newStatusText.startsWith("confirmed with")) {
                const confirmations = Number.parseInt(
                  newStatusText.split(" ")[2],
                  10,
                );

                slice.state.bobBtcLockTxConfirmations = confirmations;
              }
            }
          }
        } else if (isCliLogAliceLockedXmr(log)) {
          const nextState: SwapStateXmrLockInMempool = {
            type: SwapStateType.XMR_LOCK_TX_IN_MEMPOOL,
            aliceXmrLockTxId: log.fields.txid,
            aliceXmrLockTxConfirmations: 0,
          };

          slice.state = nextState;
        } else if (isCliLogReceivedXmrLockTxConfirmation(log)) {
          if (isSwapStateXmrLockInMempool(slice.state)) {
            if (slice.state.aliceXmrLockTxId === log.fields.txid) {
              slice.state.aliceXmrLockTxConfirmations = Number.parseInt(
                log.fields.seen_confirmations,
                10,
              );
            }
          }
        } else if (isCliLogAdvancingState(log)) {
          if (log.fields.state === "xmr is locked") {
            const nextState: SwapStateXmrLocked = {
              type: SwapStateType.XMR_LOCKED,
            };

            slice.state = nextState;
          } else if (log.fields.state === "btc is redeemed") {
            const nextState: SwapStateBtcRedemeed = {
              type: SwapStateType.BTC_REDEEMED,
            };

            slice.state = nextState;
          }
        } else if (isCliLogRedeemedXmr(log)) {
          const nextState: SwapStateXmrRedeemInMempool = {
            type: SwapStateType.XMR_REDEEM_IN_MEMPOOL,
            bobXmrRedeemTxId: log.fields.txid,
            bobXmrRedeemAddress: log.fields.monero_receive_address,
          };

          slice.state = nextState;
        } else if (isYouHaveBeenPunishedCliLog(log)) {
          const nextState: SwapStateBtcPunished = {
            type: SwapStateType.BTC_PUNISHED,
          };

          slice.state = nextState;
        } else if (isCliLogAttemptingToCooperativelyRedeemXmr(log)) {
          const nextState: SwapStateAttemptingCooperativeRedeeem = {
            type: SwapStateType.ATTEMPTING_COOPERATIVE_REDEEM,
          };

          slice.state = nextState;
        } else if (
          isCliLogReleasingSwapLockLog(log) &&
          !action.payload.isFromRestore
        ) {
          const nextState: SwapStateProcessExited = {
            type: SwapStateType.PROCESS_EXITED,
            prevState: slice.state,
            rpcError: null,
          };

          slice.state = nextState;
          slice.processRunning = false;
        } else if (isCliLogApiCallError(log) && !action.payload.isFromRestore) {
          if (isSwapStateProcessExited(slice.state)) {
            slice.state.rpcError = log.fields.err;
          }
        } else {
          logger.debug({ log }, `Swap log was not reduced`);
        }
      });
    },
    swapReset() {
      return initialState;
    },
    swapInitiate(
      swap,
      action: PayloadAction<{
        provider: Provider | null;
        spawnType: SwapSpawnType;
        swapId: string | null;
      }>,
    ) {
      const nextState: SwapStateInitiated = {
        type: SwapStateType.INITIATED,
      };

      swap.processRunning = true;
      swap.state = nextState;
      swap.logs = [];
      swap.provider = action.payload.provider;
      swap.spawnType = action.payload.spawnType;
      swap.swapId = action.payload.swapId;
    },
    swapProcessExited(swap, action: PayloadAction<string | null>) {
      if (!swap.processRunning) {
        logger.warn(`swapProcessExited called on a swap that is not running`);
        return;
      }

      const nextState: SwapStateProcessExited = {
        type: SwapStateType.PROCESS_EXITED,
        prevState: swap.state,
        rpcError: action.payload,
      };

      swap.state = nextState;
      swap.processRunning = false;
    },
  },
});

export const { swapInitiate, swapProcessExited, swapReset, swapAddLog } =
  swapSlice.actions;

export default swapSlice.reducer;
