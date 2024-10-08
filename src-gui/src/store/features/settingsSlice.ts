import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { TauriSettings } from "models/tauriModel";

const initialState: TauriSettings = {
  bitcoin_confirmation_target: 1,
  electrum_rpc_url: null,
  monero_node_url: null,
};

const alertsSlice = createSlice({
  name: "settings",
  initialState,
  reducers: {
    setBitcoinConfirmationTarget(slice, action: PayloadAction<number>) {
      slice.bitcoin_confirmation_target = action.payload;
    },
    setElectrumRpcUrl(slice, action: PayloadAction<string | null>) {
      if (action.payload === null || action.payload === "") {
        slice.electrum_rpc_url = null;
      } else {
        slice.electrum_rpc_url = action.payload;
      }
    },
    setMoneroNodeUrl(slice, action: PayloadAction<string | null>) {
      if (action.payload === null || action.payload === "") {
        slice.monero_node_url = null;
      } else {
        slice.monero_node_url = action.payload;
      }
    },
    resetSettings(slice) {
      return initialState;
    }
  },
});

export const {
  setBitcoinConfirmationTarget,
  setElectrumRpcUrl,
  setMoneroNodeUrl,
  resetSettings
} = alertsSlice.actions;
export default alertsSlice.reducer;
