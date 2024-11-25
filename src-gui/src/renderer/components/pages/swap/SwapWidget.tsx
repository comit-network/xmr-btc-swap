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
import { ExtendedMakerStatus } from "models/apiModel";
import { ChangeEvent, useEffect, useState } from "react";
import { useAppSelector } from "store/hooks";
import { satsToBtc } from "utils/conversionUtils";
import {
  ListSellersDialogOpenButton,
  MakerSubmitDialogOpenButton,
} from "../../modal/provider/MakerListDialog";
import MakerSelect from "../../modal/provider/MakerSelect";
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
  makerInfo: {
    padding: theme.spacing(1),
  },
  swapIconOuter: {
    display: "flex",
    justifyContent: "center",
  },
  swapIcon: {
    marginRight: theme.spacing(1),
  },
  noMakersAlertOuter: {
    display: "flex",
    flexDirection: "column",
    gap: theme.spacing(1),
  },
  noMakersAlertButtonsOuter: {
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

function HasMakerSwapWidget({
  selectedMaker,
}: {
  selectedMaker: ExtendedMakerStatus;
}) {
  const classes = useStyles();

  const forceShowDialog = useAppSelector((state) => state.swap.state !== null);
  const [showDialog, setShowDialog] = useState(false);
  const [btcFieldValue, setBtcFieldValue] = useState<number | string>(
    satsToBtc(selectedMaker.minSwapAmount),
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
        parsedBtcAmount / satsToBtc(selectedMaker.price);
      setXmrFieldValue(convertedXmrAmount);
    }
  }

  function getBtcFieldError(): string | null {
    const parsedBtcAmount = Number(btcFieldValue);
    if (Number.isNaN(parsedBtcAmount)) {
      return "This is not a valid number";
    }
    if (parsedBtcAmount < satsToBtc(selectedMaker.minSwapAmount)) {
      return `The minimum swap amount is ${satsToBtc(
        selectedMaker.minSwapAmount,
      )} BTC. Switch to a different maker if you want to swap less.`;
    }
    if (parsedBtcAmount > satsToBtc(selectedMaker.maxSwapAmount)) {
      return `The maximum swap amount is ${satsToBtc(
        selectedMaker.maxSwapAmount,
      )} BTC. Switch to a different maker if you want to swap more.`;
    }
    return null;
  }

  function handleGuideDialogOpen() {
    setShowDialog(true);
  }

  useEffect(updateXmrValue, [btcFieldValue, selectedMaker]);

  return (
    // 'elevation' prop can't be passed down (type def issue)
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    <Box className={classes.inner} component={Paper} elevation={5}>
      <Title />
      <TextField
        label="For this many BTC"
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
        label="You'd receive that many XMR"
        variant="outlined"
        size="medium"
        value={xmrFieldValue.toFixed(6)}
        InputProps={{
          endAdornment: <InputAdornment position="end">XMR</InputAdornment>,
        }}
      />
      <MakerSelect />
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

function HasNoMakersSwapWidget() {
  const forceShowDialog = useAppSelector((state) => state.swap.state !== null);
  const isPublicRegistryDown = useAppSelector((state) =>
    isRegistryDown(state.makers.registry.connectionFailsCount),
  );
  const classes = useStyles();

  const alertBox = isPublicRegistryDown ? (
    <Alert severity="info">
      <Box className={classes.noMakersAlertOuter}>
        <Typography>
          Currently, the public registry of makers seems to be unreachable.
          Here&apos;s what you can do:
          <ul>
            <li>
              Try discovering a maker by connecting to a rendezvous point
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
      <Box className={classes.noMakersAlertOuter}>
        <Typography>
          Currently, there are no makers (trading partners) available in the
          official registry. Here&apos;s what you can do:
          <ul>
            <li>
              Try discovering a maker by connecting to a rendezvous point
            </li>
            <li>Add a new maker to the public registry</li>
            <li>Try again later when more makers may be available</li>
          </ul>
        </Typography>
        <Box>
          <MakerSubmitDialogOpenButton />
          <ListSellersDialogOpenButton />
        </Box>
      </Box>
    </Alert>
  );

  return (
    <Box>
      {alertBox}
      <SwapDialog open={forceShowDialog} onClose={() => { }} />
    </Box>
  );
}

function MakerLoadingSwapWidget() {
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
  const selectedMaker = useAppSelector(
    (state) => state.makers.selectedMaker,
  );
  // If we fail more than RECONNECTION_ATTEMPTS_UNTIL_ASSUME_DOWN reconnect attempts, we'll show the "no makers" widget. We can assume the public registry is down.
  const makerLoading = useAppSelector(
    (state) =>
      state.makers.registry.makers === null &&
      !isRegistryDown(state.makers.registry.connectionFailsCount),
  );

  if (makerLoading) {
    return <MakerLoadingSwapWidget />;
  }
  if (selectedMaker) {
    return <HasMakerSwapWidget selectedMaker={selectedMaker} />;
  }
  return <HasNoMakersSwapWidget />;
}
