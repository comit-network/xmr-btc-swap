import {
  Box,
  Fab,
  LinearProgress,
  Paper,
  TextField,
  Typography,
} from "@mui/material";
import InputAdornment from "@mui/material/InputAdornment";
import ArrowDownwardIcon from "@mui/icons-material/ArrowDownward";
import SwapHorizIcon from "@mui/icons-material/SwapHoriz";
import { Alert } from "@mui/material";
import { ExtendedMakerStatus } from "models/apiModel";
import { ChangeEvent, useEffect, useState } from "react";
import { useAppSelector } from "store/hooks";
import { satsToBtc } from "utils/conversionUtils";
import { MakerSubmitDialogOpenButton } from "../../modal/provider/MakerListDialog";
import MakerSelect from "../../modal/provider/MakerSelect";
import SwapDialog from "../../modal/swap/SwapDialog";

// After RECONNECTION_ATTEMPTS_UNTIL_ASSUME_DOWN failed reconnection attempts we can assume the public registry is down
const RECONNECTION_ATTEMPTS_UNTIL_ASSUME_DOWN = 1;

function isRegistryDown(reconnectionAttempts: number): boolean {
  return reconnectionAttempts > RECONNECTION_ATTEMPTS_UNTIL_ASSUME_DOWN;
}

function Title() {
  return (
    <Box sx={{ padding: 0 }}>
      <Typography variant="h5" sx={{ padding: 1 }}>
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
    <Paper
      variant="outlined"
      sx={(theme) => ({
        width: "min(480px, 100%)",
        minHeight: "150px",
        display: "grid",
        padding: theme.spacing(2),
        gridGap: theme.spacing(1),
      })}
    >
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
      <Box sx={{ display: "flex", justifyContent: "center" }}>
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
        <SwapHorizIcon sx={{ marginRight: 1 }} />
        Swap
      </Fab>
      <SwapDialog
        open={showDialog || forceShowDialog}
        onClose={() => setShowDialog(false)}
      />
    </Paper>
  );
}

function HasNoMakersSwapWidget() {
  const forceShowDialog = useAppSelector((state) => state.swap.state !== null);
  const isPublicRegistryDown = useAppSelector((state) =>
    isRegistryDown(state.makers.registry.connectionFailsCount),
  );

  const alertBox = isPublicRegistryDown ? (
    <Alert severity="info">
      <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
        <Typography>
          Currently, the public registry of makers seems to be unreachable.
          Here&apos;s what you can do:
          <ul>
            <li>Try discovering a maker by connecting to a rendezvous point</li>
            <li>
              Try again later when the public registry may be reachable again
            </li>
          </ul>
        </Typography>
        <Box>
          <MakerSubmitDialogOpenButton />
        </Box>
      </Box>
    </Alert>
  ) : (
    <Alert severity="info">
      <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
        <Typography>
          Currently, there are no makers (trading partners) available in the
          official registry. Here&apos;s what you can do:
          <ul>
            <li>Try discovering a maker by connecting to a rendezvous point</li>
            <li>Add a new maker to the public registry</li>
            <li>Try again later when more makers may be available</li>
          </ul>
        </Typography>
        <Box sx={{ display: "flex", gap: 1 }}>
          <MakerSubmitDialogOpenButton />
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

function MakerLoadingSwapWidget() {
  return (
    // 'elevation' prop can't be passed down (type def issue)
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    <Box
      component={Paper}
      elevation={15}
      sx={{
        width: "min(480px, 100%)",
        minHeight: "150px",
        display: "grid",
        padding: 1,
        gridGap: 1,
      }}
    >
      <Title />
      <LinearProgress />
    </Box>
  );
}

export default function SwapWidget() {
  const selectedMaker = useAppSelector((state) => state.makers.selectedMaker);
  // If we fail more than RECONNECTION_ATTEMPTS_UNTIL_ASSUME_DOWN reconnect attempts, we'll show the "no makers" widget. We can assume the public registry is down.
  const makerLoading = useAppSelector(
    (state) =>
      state.makers.registry.makers === null &&
      !isRegistryDown(state.makers.registry.connectionFailsCount),
  );

  if (makerLoading) {
    return <MakerLoadingSwapWidget />;
  }

  if (selectedMaker === null) {
    return <HasNoMakersSwapWidget />;
  }

  return <HasMakerSwapWidget selectedMaker={selectedMaker} />;
}
