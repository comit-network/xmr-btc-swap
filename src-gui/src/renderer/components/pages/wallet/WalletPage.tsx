import { Box, makeStyles, Typography } from "@material-ui/core";
import { Alert } from "@material-ui/lab";
import WithdrawWidget from "./WithdrawWidget";

const useStyles = makeStyles((theme) => ({
  outer: {
    display: "flex",
    flexDirection: "column",
    gridGap: theme.spacing(0.5),
  },
}));

export default function WalletPage() {
  const classes = useStyles();

  return (
    <Box className={classes.outer}>
      <Typography variant="h3">Wallet</Typography>
      <Alert severity="info">
        You do not have to deposit money before starting a swap. Instead, you
        will be greeted with a deposit address after you initiate one.
      </Alert>
      <Typography variant="subtitle1">
        If funds are left in your wallet after a swap, you can withdraw them to
        your wallet. If you decide to leave them inside the internal wallet, the
        funds will automatically be used when starting a new swap.
      </Typography>
      <WithdrawWidget />
    </Box>
  );
}
