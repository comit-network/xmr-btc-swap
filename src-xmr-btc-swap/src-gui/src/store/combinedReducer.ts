import swapReducer from './features/swapSlice';
import providersSlice from './features/providersSlice';
import torSlice from './features/torSlice';
import rpcSlice from './features/rpcSlice';
import alertsSlice from './features/alertsSlice';
import ratesSlice from './features/ratesSlice';

export const reducers = {
  swap: swapReducer,
  providers: providersSlice,
  tor: torSlice,
  rpc: rpcSlice,
  alerts: alertsSlice,
  rates: ratesSlice,
};
