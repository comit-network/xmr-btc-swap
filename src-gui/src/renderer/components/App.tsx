import { Box, CssBaseline, makeStyles } from "@material-ui/core";
import { ThemeProvider } from "@material-ui/core/styles";
import "@tauri-apps/plugin-shell";
import { Route, MemoryRouter as Router, Routes } from "react-router-dom";
import Navigation, { drawerWidth } from "./navigation/Navigation";
import HelpPage from "./pages/help/HelpPage";
import HistoryPage from "./pages/history/HistoryPage";
import SwapPage from "./pages/swap/SwapPage";
import WalletPage from "./pages/wallet/WalletPage";
import GlobalSnackbarProvider from "./snackbar/GlobalSnackbarProvider";
import UpdaterDialog from "./modal/updater/UpdaterDialog";
import { useSettings } from "store/hooks";
import { themes } from "./theme";
import { initEventListeners, updateAllNodeStatuses } from "renderer/rpc";
import { fetchAlertsViaHttp, fetchProvidersViaHttp, updateRates } from "renderer/api";
import { store } from "renderer/store/storeRenderer";
import logger from "utils/logger";
import { setAlerts } from "store/features/alertsSlice";
import { setRegistryProviders } from "store/features/providersSlice";
import { registryConnectionFailed } from "store/features/providersSlice";
import { useEffect } from "react";

const useStyles = makeStyles((theme) => ({
  innerContent: {
    padding: theme.spacing(4),
    marginLeft: drawerWidth,
    maxHeight: `100vh`,
    flex: 1,
  },
}));

export default function App() {
  useEffect(() => {
    fetchInitialData();
    initEventListeners();
  }, []);

  const theme = useSettings((s) => s.theme);

  return (
    <ThemeProvider theme={themes[theme]}>
      <GlobalSnackbarProvider>
        <CssBaseline />
        <Router>
          <Navigation />
          <InnerContent />
          <UpdaterDialog />
        </Router>
      </GlobalSnackbarProvider>
    </ThemeProvider>
  );
}

function InnerContent() {
  const classes = useStyles();

  return (
    <Box className={classes.innerContent}>
      <Routes>
        <Route path="/swap" element={<SwapPage />} />
        <Route path="/history" element={<HistoryPage />} />
        <Route path="/wallet" element={<WalletPage />} />
        <Route path="/help" element={<HelpPage />} />
        <Route path="/" element={<SwapPage />} />
      </Routes>
    </Box>
  );
}

async function fetchInitialData() {
  try {
    const providerList = await fetchProvidersViaHttp();
    store.dispatch(setRegistryProviders(providerList));

    logger.info(
      { providerList },
      "Fetched providers via UnstoppableSwap HTTP API",
    );
  } catch (e) {
    store.dispatch(registryConnectionFailed());
    logger.error(e, "Failed to fetch providers via UnstoppableSwap HTTP API");
  }

  try {
    await updateAllNodeStatuses()
  } catch (e) {
    logger.error(e, "Failed to update node statuses")
  }

  // Update node statuses every 2 minutes
  const STATUS_UPDATE_INTERVAL = 2 * 60 * 1_000;
  setInterval(updateAllNodeStatuses, STATUS_UPDATE_INTERVAL);

  try {
    const alerts = await fetchAlertsViaHttp();
    store.dispatch(setAlerts(alerts));
    logger.info({ alerts }, "Fetched alerts via UnstoppableSwap HTTP API");
  } catch (e) {
    logger.error(e, "Failed to fetch alerts via UnstoppableSwap HTTP API");
  }

  try {
    await updateRates();
    logger.info("Fetched XMR/BTC rate");
  } catch (e) {
    logger.error(e, "Error retrieving XMR/BTC rate");
  }
  
  // Update the rates every 5 minutes (to respect the coingecko rate limit)
  const RATE_UPDATE_INTERVAL = 5 * 60 * 1_000;
  setInterval(updateRates, RATE_UPDATE_INTERVAL);
}
