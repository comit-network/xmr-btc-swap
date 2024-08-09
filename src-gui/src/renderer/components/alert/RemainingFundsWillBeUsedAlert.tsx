import { Alert } from "@material-ui/lab";
import { Box, makeStyles } from "@material-ui/core";
import { useAppSelector } from "store/hooks";
import WalletRefreshButton from "../pages/wallet/WalletRefreshButton";
import { SatsAmount } from "../other/Units";

const useStyles = makeStyles((theme) => ({
  outer: {
    paddingBottom: theme.spacing(1),
  },
}));

export default function RemainingFundsWillBeUsedAlert() {
  const classes = useStyles();
  const balance = useAppSelector((s) => s.rpc.state.balance);

  if (balance == null || balance <= 0) {
    return <></>;
  }

  return (
    <Box className={classes.outer}>
      <Alert
        severity="warning"
        action={<WalletRefreshButton />}
        variant="filled"
      >
        The remaining funds of <SatsAmount amount={balance} /> in the wallet
        will be used for the next swap. If the remaining funds exceed the
        minimum swap amount of the provider, a swap will be initiated
        instantaneously.
      </Alert>
    </Box>
  );
}
