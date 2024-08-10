import { Box, makeStyles } from "@material-ui/core";
import { useSwapInfosSortedByDate } from "store/hooks";
import SwapStatusAlert from "./SwapStatusAlert";

const useStyles = makeStyles((theme) => ({
  outer: {
    display: "flex",
    flexDirection: "column",
    gap: theme.spacing(1),
  },
}));

export default function SwapTxLockAlertsBox() {
  const classes = useStyles();

  // We specifically choose ALL swaps here
  // If a swap is in a state where an Alert is not needed (becaue no Bitcoin have been locked or because the swap has been completed)
  // the SwapStatusAlert component will not render an Alert
  const swaps = useSwapInfosSortedByDate();

  return (
    <Box className={classes.outer}>
      {swaps.map((swap) => (
        <SwapStatusAlert key={swap.swap_id} swap={swap} />
      ))}
    </Box>
  );
}
