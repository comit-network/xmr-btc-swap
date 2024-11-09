import { createRoot } from "react-dom/client";
import { Provider } from "react-redux";
import { PersistGate } from "redux-persist/integration/react";
import { setAlerts } from "store/features/alertsSlice";
import {
  registryConnectionFailed,
  setRegistryProviders,
} from "store/features/providersSlice";
import { setBtcPrice, setXmrBtcRate, setXmrPrice } from "store/features/ratesSlice";
import logger from "../utils/logger";
import {
  fetchAlertsViaHttp,
  fetchBtcPrice,
  fetchProvidersViaHttp,
  fetchXmrBtcRate,
  fetchXmrPrice,
} from "./api";
import App from "./components/App";
import { initEventListeners } from "./rpc";
import { persistor, store } from "./store/storeRenderer";

const container = document.getElementById("root");
const root = createRoot(container!);

root.render(
  <Provider store={store}>
    <PersistGate loading={null} persistor={persistor}>
      <App />
    </PersistGate>
  </Provider>,
);