import {
  Box,
  Fab,
  LinearProgress,
  makeStyles,
  Paper,
  TextField,
  Typography,
} from "@material-ui/core";
import InputAdornment from "@material-ui/core/InputAdornment";
import ArrowDownwardIcon from "@material-ui/icons/ArrowDownward";
import SwapHorizIcon from "@material-ui/icons/SwapHoriz";
import { Alert } from "@material-ui/lab";
import { ExtendedProviderStatus } from "models/apiModel";
import { ChangeEvent, useEffect, useState } from "react";
import { useAppSelector } from "store/hooks";
import { satsToBtc } from "utils/conversionUtils";
import {
  ListSellersDialogOpenButton,
  ProviderSubmitDialogOpenButton,
} from "../../modal/provider/ProviderListDialog";
import ProviderSelect from "../../modal/provider/ProviderSelect";
import SwapDialog from "../../modal/swap/SwapDialog";

// After RECONNECTION_ATTEMPTS_UNTIL_ASSUME_DOWN failed reconnection attempts we can assume the public registry is down
const RECONNECTION_ATTEMPTS_UNTIL_ASSUME_DOWN = 1;

function isRegistryDown(reconnectionAttempts: number): boolean {
  return reconnectionAttempts > RECONNECTION_ATTEMPTS_UNTIL_ASSUME_DOWN;
}

const useStyles = makeStyles((theme) => ({
  inner: {
    width: "min(480px, 100%)",
    minHeight: "150px",
    display: "grid",
    padding: theme.spacing(1),
    gridGap: theme.spacing(1),
  },
  header: {
    padding: 0,
  },
  headerText: {
    padding: theme.spacing(1),
  },
  providerInfo: {
    padding: theme.spacing(1),
  },
  swapIconOuter: {
    display: "flex",
    justifyContent: "center",
  },
  swapIcon: {
    marginRight: theme.spacing(1),
  },
  noProvidersAlertOuter: {
    display: "flex",
    flexDirection: "column",
    gap: theme.spacing(1),
  },
  noProvidersAlertButtonsOuter: {
    display: "flex",
    gap: theme.spacing(1),
  },
}));

function Title() {
  const classes = useStyles();

  return (
    <Box className={classes.header}>
      <Typography variant="h5" className={classes.headerText}>
        Swap
      </Typography>
    </Box>
  );
}

function HasProviderSwapWidget({
  selectedProvider,
}: {
  selectedProvider: ExtendedProviderStatus;
}) {
  const classes = useStyles();

  const forceShowDialog = useAppSelector((state) => state.swap.state !== null);
  const [showDialog, setShowDialog] = useState(false);
  const [btcFieldValue, setBtcFieldValue] = useState<number | string>(
    satsToBtc(selectedProvider.minSwapAmount),
  );
  const [xmrFieldValue, setXmrFieldValue] = useState(1);

  function onBtcAmountChange(event: ChangeEvent<HTMLInputElement>) {
    setBtcFieldValue(event.target.value);
  }

  function updateXmrValue() {
    const parsedBtcAmount = Number(btcFieldValue);
    if (Number.isNaN(parsedBtcAmount)) {
      setXmrFieldValue(0);
    } else {
      const convertedXmrAmount =
        parsedBtcAmount / satsToBtc(selectedProvider.price);
      setXmrFieldValue(convertedXmrAmount);
    }
  }

  function getBtcFieldError(): string | null {
    const parsedBtcAmount = Number(btcFieldValue);
    if (Number.isNaN(parsedBtcAmount)) {
      return "This is not a valid number";
    }
    if (parsedBtcAmount < satsToBtc(selectedProvider.minSwapAmount)) {
      return `The minimum swap amount is ${satsToBtc(
        selectedProvider.minSwapAmount,
      )} BTC. Switch to a different provider if you want to swap less.`;
    }
    if (parsedBtcAmount > satsToBtc(selectedProvider.maxSwapAmount)) {
      return `The maximum swap amount is ${satsToBtc(
        selectedProvider.maxSwapAmount,
      )} BTC. Switch to a different provider if you want to swap more.`;
    }
    return null;
  }

  function handleGuideDialogOpen() {
    setShowDialog(true);
  }

  useEffect(updateXmrValue, [btcFieldValue, selectedProvider]);

  return (
    // 'elevation' prop can't be passed down (type def issue)
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    <Box className={classes.inner} component={Paper} elevation={5}>
      <Title />
      <TextField
        label="Send"
        size="medium"
        variant="outlined"
        value={btcFieldValue}
        onChange={onBtcAmountChange}
        error={!!getBtcFieldError()}
        helperText={getBtcFieldError()}
        autoFocus
        InputProps={{
          endAdornment: <InputAdornment position="end">BTC</InputAdornment>,
        }}
      />
      <Box className={classes.swapIconOuter}>
        <ArrowDownwardIcon fontSize="small" />
      </Box>
      <TextField
        label="Receive"
        variant="outlined"
        size="medium"
        value={xmrFieldValue.toFixed(6)}
        InputProps={{
          endAdornment: <InputAdornment position="end">XMR</InputAdornment>,
        }}
      />
      <ProviderSelect />
      <Fab variant="extended" color="primary" onClick={handleGuideDialogOpen}>
        <SwapHorizIcon className={classes.swapIcon} />
        Swap
      </Fab>
      <SwapDialog
        open={showDialog || forceShowDialog}
        onClose={() => setShowDialog(false)}
      />
    </Box>
  );
}

function HasNoProvidersSwapWidget() {
  const forceShowDialog = useAppSelector((state) => state.swap.state !== null);
  const isPublicRegistryDown = useAppSelector((state) =>
    isRegistryDown(
      state.providers.registry.connectionFailsCount,
    ),
  );
  const classes = useStyles();

  const alertBox = isPublicRegistryDown ? (
    <Alert severity="info">
      <Box className={classes.noProvidersAlertOuter}>
        <Typography>
          Currently, the public registry of providers seems to be unreachable.
          Here&apos;s what you can do:
          <ul>
            <li>
              Try discovering a provider by connecting to a rendezvous point
            </li>
            <li>
              Try again later when the public registry may be reachable again
            </li>
          </ul>
        </Typography>
        <Box>
          <ListSellersDialogOpenButton />
        </Box>
      </Box>
    </Alert>
  ) : (
    <Alert severity="info">
      <Box className={classes.noProvidersAlertOuter}>
        <Typography>
          Currently, there are no providers (trading partners) available in the
          official registry. Here&apos;s what you can do:
          <ul>
            <li>
              Try discovering a provider by connecting to a rendezvous point
            </li>
            <li>Add a new provider to the public registry</li>
            <li>Try again later when more providers may be available</li>
          </ul>
        </Typography>
        <Box>
          <ProviderSubmitDialogOpenButton />
          <ListSellersDialogOpenButton />
        </Box>
      </Box>
    </Alert>
  );

  return (
    <Box>
      {alertBox}
      <SwapDialog open={forceShowDialog} onClose={() => {}} />
    </Box>
  );
}

function ProviderLoadingSwapWidget() {
  const classes = useStyles();

  return (
    // 'elevation' prop can't be passed down (type def issue)
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    <Box className={classes.inner} component={Paper} elevation={15}>
      <Title />
      <LinearProgress />
    </Box>
  );
}

export default function SwapWidget() {
  const selectedProvider = useAppSelector(
    (state) => state.providers.selectedProvider,
  );
  // If we fail more than RECONNECTION_ATTEMPTS_UNTIL_ASSUME_DOWN reconnect attempts, we'll show the "no providers" widget. We can assume the public registry is down.
  const providerLoading = useAppSelector(
    (state) =>
      state.providers.registry.providers === null &&
      !isRegistryDown(
        state.providers.registry.connectionFailsCount,
      ),
  );

  if (providerLoading) {
    return <ProviderLoadingSwapWidget />;
  }
  if (selectedProvider) {
    return <HasProviderSwapWidget selectedProvider={selectedProvider} />;
  }
  return <HasNoProvidersSwapWidget />;
}
