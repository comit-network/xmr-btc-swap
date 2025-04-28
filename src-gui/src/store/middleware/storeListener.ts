import { createListenerMiddleware } from "@reduxjs/toolkit";
import { getAllSwapInfos, checkBitcoinBalance, updateAllNodeStatuses, fetchSellersAtPresetRendezvousPoints, getSwapInfo } from "renderer/rpc";
import logger from "utils/logger";
import { contextStatusEventReceived } from "store/features/rpcSlice";
import { addNode, setFetchFiatPrices, setFiatCurrency } from "store/features/settingsSlice";
import { fetchFeedbackMessagesViaHttp, updateRates } from "renderer/api";
import { store } from "renderer/store/storeRenderer";
import { swapProgressEventReceived } from "store/features/swapSlice";
import { addFeedbackId, setConversation } from "store/features/conversationsSlice";

export function createMainListeners() {
  const listener = createListenerMiddleware();

  // Listener for when the Context becomes available
  // When the context becomes available, we check the bitcoin balance, fetch all swap infos and connect to the rendezvous point
  listener.startListening({
    actionCreator: contextStatusEventReceived,
    effect: async (action) => {
      const status = action.payload;

      // If the context is available, check the bitcoin balance and fetch all swap infos
      if (status.type === "Available") {
        logger.debug(
          "Context is available, checking bitcoin balance and history",
        );
        await Promise.allSettled([
          checkBitcoinBalance(),
          getAllSwapInfos(),
          fetchSellersAtPresetRendezvousPoints(),
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

      // Update the swap info
      logger.info("Swap progress event received, updating swap info from database...");
      await getSwapInfo(action.payload.swap_id);
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
