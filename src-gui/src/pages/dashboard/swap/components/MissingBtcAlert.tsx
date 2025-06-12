import { Alert } from "@mui/material";
import { useAppSelector } from "store/hooks";

export default function MissingBtcAlert() {
  const btcAmount = useAppSelector((state) => state.startSwap.btcAmount);
  const btcBalance = useAppSelector((state) => state.rpc.state.balance);

  if (btcAmount <= btcBalance) {
    return null;
  }

  return (
    <Alert severity="info" variant="outlined">
      Your Wallet has {btcBalance} BTC. You need an additional {btcAmount - btcBalance} BTC to swap your desired XMR amount.
    </Alert>
  );
}