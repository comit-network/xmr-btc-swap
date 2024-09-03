import { createListenerMiddleware } from "@reduxjs/toolkit";
import { getAllSwapInfos, checkBitcoinBalance } from "renderer/rpc";
import logger from "utils/logger";
import { contextStatusEventReceived } from "store/features/rpcSlice";

export function createMainListeners() {
  const listener = createListenerMiddleware();

  // Listener for when the Context becomes available
  // When the context becomes available, we check the bitcoin balance and fetch all swap infos
  listener.startListening({
    actionCreator: contextStatusEventReceived,
    effect: async (action) => {
      const status = action.payload;

      // If the context is available, check the bitcoin balance and fetch all swap infos
      if (status.type === "Available") {
        logger.debug(
          "Context is available, checking bitcoin balance and history",
        );
        await checkBitcoinBalance();
        await getAllSwapInfos();
      }
    },
  });

  return listener;
}
