import { combineReducers, configureStore } from "@reduxjs/toolkit";
import { persistReducer, persistStore } from "redux-persist";
import sessionStorage from "redux-persist/lib/storage/session";
import { reducers } from "store/combinedReducer";

// We persist the redux store in sessionStorage
// The point of this is to preserve the store across reloads while not persisting it across GUI restarts
//
// If the user reloads the page, while a swap is running we want to
// continue displaying the correct state of the swap
const persistConfig = {
  key: "gui-global-state-store",
  storage: sessionStorage,
};

const persistedReducer = persistReducer(
  persistConfig,
  combineReducers(reducers),
);

export const store = configureStore({
  reducer: persistedReducer,
});

export const persistor = persistStore(store);

export type AppDispatch = typeof store.dispatch;
export type RootState = ReturnType<typeof store.getState>;
