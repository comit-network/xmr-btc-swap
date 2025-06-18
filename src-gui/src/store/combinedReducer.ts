import alertsSlice from "./features/alertsSlice";
import makersSlice from "./features/makersSlice";
import ratesSlice from "./features/ratesSlice";
import rpcSlice from "./features/rpcSlice";
import swapReducer from "./features/swapSlice";
import torSlice from "./features/torSlice";
import settingsSlice from "./features/settingsSlice";
import nodesSlice from "./features/nodesSlice";
import conversationsSlice from "./features/conversationsSlice";
import poolSlice from "./features/poolSlice";

export const reducers = {
  swap: swapReducer,
  makers: makersSlice,
  tor: torSlice,
  rpc: rpcSlice,
  alerts: alertsSlice,
  rates: ratesSlice,
  settings: settingsSlice,
  nodes: nodesSlice,
  conversations: conversationsSlice,
  pool: poolSlice,
};
