import alertsSlice from "./features/alertsSlice";
import providersSlice from "./features/providersSlice";
import ratesSlice from "./features/ratesSlice";
import rpcSlice from "./features/rpcSlice";
import swapReducer from "./features/swapSlice";
import torSlice from "./features/torSlice";

export const reducers = {
  swap: swapReducer,
  providers: providersSlice,
  tor: torSlice,
  rpc: rpcSlice,
  alerts: alertsSlice,
  rates: ratesSlice,
};
