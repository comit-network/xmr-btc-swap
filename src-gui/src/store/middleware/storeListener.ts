import { createListenerMiddleware } from "@reduxjs/toolkit";
import { throttle, debounce } from "lodash";
import {
  getAllSwapInfos,
  checkBitcoinBalance,
  updateAllNodeStatuses,
  fetchSellersAtPresetRendezvousPoints,
  getSwapInfo,
  initializeMoneroWallet,
} from "renderer/rpc";
import logger from "utils/logger";
import { contextStatusEventReceived } from "store/features/rpcSlice";
import {
  addNode,
  setFetchFiatPrices,
  setFiatCurrency,
} from "store/features/settingsSlice";
import { fetchFeedbackMessagesViaHttp, updateRates } from "renderer/api";
import { store } from "renderer/store/storeRenderer";
import { swapProgressEventReceived } from "store/features/swapSlice";
import {
  addFeedbackId,
  setConversation,
} from "store/features/conversationsSlice";
import { TauriContextStatusEvent } from "models/tauriModel";

// Create a Map to store throttled functions per swap_id
const throttledGetSwapInfoFunctions = new Map<
  string,
  ReturnType<typeof throttle>
>();

// Function to get or create a throttled getSwapInfo for a specific swap_id
const getThrottledSwapInfoUpdater = (swapId: string) => {
  if (!throttledGetSwapInfoFunctions.has(swapId)) {
    // Create a throttled function that executes at most once every 2 seconds
    // but will wait for 3 seconds of quiet during rapid calls (using debounce)
    const debouncedGetSwapInfo = debounce(() => {
      logger.debug(`Executing getSwapInfo for swap ${swapId}`);
      getSwapInfo(swapId);
    }, 3000); // 3 seconds debounce for rapid calls

    const throttledFunction = throttle(debouncedGetSwapInfo, 2000, {
      leading: true, // Execute immediately on first call
      trailing: true, // Execute on trailing edge if needed
    });

    throttledGetSwapInfoFunctions.set(swapId, throttledFunction);
  }

  return throttledGetSwapInfoFunctions.get(swapId)!;
};

export function createMainListeners() {
  const listener = createListenerMiddleware();

  // Listener for when the Context becomes available
  // When the context becomes available, we check the bitcoin balance, fetch all swap infos and connect to the rendezvous point
  listener.startListening({
    actionCreator: contextStatusEventReceived,
    effect: async (action) => {
      const status = action.payload;

      // If the context is available, check the Bitcoin balance and fetch all swap infos
      if (status === TauriContextStatusEvent.Available) {
        logger.debug(
          "Context is available, checking Bitcoin balance and history",
        );
        await Promise.allSettled([
          checkBitcoinBalance(),
          getAllSwapInfos(),
          fetchSellersAtPresetRendezvousPoints(),
          initializeMoneroWallet(),
        ]);
      }
    },
  });

  // Listener for:
  // - when a swap is released (fetch bitcoin balance)
  // - when a swap progress event is received (update the swap info)
  listener.startListening({
    actionCreator: swapProgressEventReceived,
    effect: async (action) => {
      if (action.payload.event.type === "Released") {
        logger.info("Swap released, updating bitcoin balance...");
        await checkBitcoinBalance();
      }

      // Update the swap info using throttled function
      logger.info(
        "Swap progress event received, scheduling throttled swap info update...",
      );
      const throttledUpdater = getThrottledSwapInfoUpdater(
        action.payload.swap_id,
      );
      throttledUpdater();
    },
  });

  // Update the rates when the fiat currency is changed
  listener.startListening({
    actionCreator: setFiatCurrency,
    effect: async () => {
      if (store.getState().settings.fetchFiatPrices) {
        logger.info("Fiat currency changed, updating rates...");
        await updateRates();
      }
    },
  });

  // Update the rates when fetching fiat prices is enabled
  listener.startListening({
    actionCreator: setFetchFiatPrices,
    effect: async (action) => {
      if (action.payload === true) {
        logger.info("Activated fetching fiat prices, updating rates...");
        await updateRates();
      }
    },
  });

  // Update the node status when a new one is added
  listener.startListening({
    actionCreator: addNode,
    effect: async (_) => {
      await updateAllNodeStatuses();
    },
  });

  // Listener for when a feedback id is added
  listener.startListening({
    actionCreator: addFeedbackId,
    effect: async (action) => {
      // Whenever a new feedback id is added, fetch the messages and store them in the Redux store
      const messages = await fetchFeedbackMessagesViaHttp(action.payload);
      store.dispatch(setConversation({ feedbackId: action.payload, messages }));
    },
  });

  return listener;
}
