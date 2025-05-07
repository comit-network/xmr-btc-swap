import { Box, CssBaseline, makeStyles } from "@material-ui/core";
import { ThemeProvider } from "@material-ui/core/styles";
import "@tauri-apps/plugin-shell";
import { Route, MemoryRouter as Router, Routes } from "react-router-dom";
import Navigation, { drawerWidth } from "./navigation/Navigation";
import SettingsPage from "./pages/help/SettingsPage";
import HistoryPage from "./pages/history/HistoryPage";
import SwapPage from "./pages/swap/SwapPage";
import WalletPage from "./pages/wallet/WalletPage";
import GlobalSnackbarProvider from "./snackbar/GlobalSnackbarProvider";
import UpdaterDialog from "./modal/updater/UpdaterDialog";
import { useSettings } from "store/hooks";
import { themes } from "./theme";
import { useEffect } from "react";
import { setupBackgroundTasks } from "renderer/background";
import "@fontsource/roboto";
import FeedbackPage from "./pages/feedback/FeedbackPage";
import IntroductionModal from "./modal/introduction/IntroductionModal";

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
    setupBackgroundTasks();
  }, []);

  const theme = useSettings((s) => s.theme);

  return (
    <ThemeProvider theme={themes[theme]}>
      <GlobalSnackbarProvider>
        <CssBaseline />
        <IntroductionModal/>
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
        <Route path="/settings" element={<SettingsPage />} />
        <Route path="/feedback" element={<FeedbackPage />} />
        <Route path="/" element={<SwapPage />} />
      </Routes>
    </Box>
  );
}