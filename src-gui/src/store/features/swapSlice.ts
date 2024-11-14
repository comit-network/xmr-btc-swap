import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { TauriSwapProgressEventWrapper } from "models/tauriModel";
import { SwapSlice } from "../../models/storeModel";

const initialState: SwapSlice = {
  state: null,
  logs: [],

  // TODO: Remove this and replace logic entirely with Tauri events
  spawnType: null,
};

export const swapSlice = createSlice({
  name: "swap",
  initialState,
  reducers: {
    swapProgressEventReceived(
      swap,
      action: PayloadAction<TauriSwapProgressEventWrapper>,
    ) {
      // If either:
      // 1. No swap is currently running
      // 2. The swap ID of the event does not match the current swap ID
      //
      // Then we create a new swap state object that stores the current and previous events
      if (swap.state === null || action.payload.swap_id !== swap.state.swapId) {
        swap.state = {
          curr: action.payload.event,
          prev: null,
          swapId: action.payload.swap_id,
        };
      } else {
        swap.state.prev = swap.state.curr;
        swap.state.curr = action.payload.event;
      }
    },
    swapReset() {
      return initialState;
    },
  },
});

export const { swapReset, swapProgressEventReceived } = swapSlice.actions;

export default swapSlice.reducer;
