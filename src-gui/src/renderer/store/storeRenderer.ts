import { combineReducers, configureStore } from "@reduxjs/toolkit";
import { persistReducer, persistStore } from "redux-persist";
import sessionStorage from "redux-persist/lib/storage/session";
import { reducers } from "store/combinedReducer";
import { createMainListeners } from "store/middleware/storeListener";
import { createStore } from "@tauri-apps/plugin-store";
import { getNetworkName } from "store/config";

// Goal: Maintain application state across page reloads while allowing a clean slate on application restart
// Settings are persisted across application restarts, while the rest of the state is cleared

// Persist user settings across application restarts
// We use Tauri's storage for settings to ensure they're retained even when the application is closed
const rootPersistConfig = {
  key: "gui-global-state-store",
  storage: sessionStorage,
  blacklist: ["settings"],
};

// Use Tauri's store plugin for persistent settings
const tauriStore = await createStore(`${getNetworkName()}_settings.bin`, {
  autoSave: 1000 as unknown as boolean,
});

// Configure how settings are stored and retrieved using Tauri's storage
const settingsPersistConfig = {
  key: "settings",
  storage: {
    getItem: (key: string) => tauriStore.get(key),
    setItem: (key: string, value: unknown) => tauriStore.set(key, value),
    removeItem: (key: string) => tauriStore.delete(key),
  },
};

// Create a persisted version of the settings reducer
const persistedSettingsReducer = persistReducer(
  settingsPersistConfig,
  reducers.settings,
);

// Combine all reducers, using the persisted settings reducer
const rootReducer = combineReducers({
  ...reducers,
  settings: persistedSettingsReducer,
});

// Enable persistence for the entire application state
const persistedReducer = persistReducer(rootPersistConfig, rootReducer);

// Set up the Redux store with persistence and custom middleware
export const store = configureStore({
  reducer: persistedReducer,
  middleware: (getDefaultMiddleware) =>
    getDefaultMiddleware({
      // Disable serializable to silence warnings about non-serializable actions
      serializableCheck: false,
    }).prepend(createMainListeners().middleware),
});

// Create a persistor to manage the persisted store
export const persistor = persistStore(store);

// TypeScript type definitions for easier use of the store in the application
export type AppDispatch = typeof store.dispatch;
export type RootState = ReturnType<typeof store.getState>;