import { Box, CssBaseline, makeStyles } from "@material-ui/core";
import { indigo } from "@material-ui/core/colors";
import { createTheme, ThemeProvider } from "@material-ui/core/styles";
import "@tauri-apps/plugin-shell";
import { Route, MemoryRouter as Router, Routes } from "react-router-dom";
import Navigation, { drawerWidth } from "./navigation/Navigation";
import HelpPage from "./pages/help/HelpPage";
import HistoryPage from "./pages/history/HistoryPage";
import SwapPage from "./pages/swap/SwapPage";
import WalletPage from "./pages/wallet/WalletPage";
import GlobalSnackbarProvider from "./snackbar/GlobalSnackbarProvider";
import { useEffect } from "react";
import { fetchProvidersViaHttp, fetchAlertsViaHttp, fetchXmrPrice, fetchBtcPrice, fetchXmrBtcRate } from "renderer/api";
import { initEventListeners } from "renderer/rpc";
import { store } from "renderer/store/storeRenderer";
import UpdaterDialog from "./modal/updater/UpdaterDialog";
import { setAlerts } from "store/features/alertsSlice";
import { setRegistryProviders, registryConnectionFailed } from "store/features/providersSlice";
import { setXmrPrice, setBtcPrice, setXmrBtcRate } from "store/features/ratesSlice";
import logger from "utils/logger";

const useStyles = makeStyles((theme) => ({
  innerContent: {
    padding: theme.spacing(4),
    marginLeft: drawerWidth,
    maxHeight: `100vh`,
    flex: 1,
  },
}));

const theme = createTheme({
  palette: {
    type: "dark",
    primary: {
      main: "#f4511e",
    },
    secondary: indigo,
  },
  typography: {
    overline: {
      textTransform: "none", // This prevents the text from being all caps
    },
  },
});

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

export default function App() {
  useEffect(() => {
    fetchInitialData();
    initEventListeners();
  }, []);

  return (
    <ThemeProvider theme={theme}>
      <GlobalSnackbarProvider>
        <CssBaseline />
        <Router>
          <Navigation />
          <InnerContent />
          <UpdaterDialog/>
        </Router>
      </GlobalSnackbarProvider>
    </ThemeProvider>
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
    const alerts = await fetchAlertsViaHttp();
    store.dispatch(setAlerts(alerts));
    logger.info({ alerts }, "Fetched alerts via UnstoppableSwap HTTP API");
  } catch (e) {
    logger.error(e, "Failed to fetch alerts via UnstoppableSwap HTTP API");
  }

  try {
    const xmrPrice = await fetchXmrPrice();
    store.dispatch(setXmrPrice(xmrPrice));
    logger.info({ xmrPrice }, "Fetched XMR price");

    const btcPrice = await fetchBtcPrice();
    store.dispatch(setBtcPrice(btcPrice));
    logger.info({ btcPrice }, "Fetched BTC price");
  } catch (e) {
    logger.error(e, "Error retrieving fiat prices");
  }

  try {
    const xmrBtcRate = await fetchXmrBtcRate();
    store.dispatch(setXmrBtcRate(xmrBtcRate));
    logger.info({ xmrBtcRate }, "Fetched XMR/BTC rate");
  } catch (e) {
    logger.error(e, "Error retrieving XMR/BTC rate");
  }
}
