import { Alert, Button } from "@mui/material";
import { useAppDispatch, useAppSelector } from "store/hooks";
import { setStep, StartSwapStep } from "store/features/startSwapSlice";

export default function MissingBtcAlert() {
  const btcAmount = useAppSelector((state) => state.startSwap.btcAmount);
  const step = useAppSelector((state) => state.startSwap.step)
  const btcBalance = useAppSelector((state) => state.rpc.state.balance);
  const dispatch = useAppDispatch();
  if (btcAmount <= btcBalance) {
    return null;
  }

  return (
    <Alert severity="info" variant="outlined"
    action={step != StartSwapStep.DepositBitcoin &&
        <Button color="inherit" size="small" onClick={() => {
            dispatch(setStep(StartSwapStep.DepositBitcoin));
        }}>
            Get Bitcoin
        </Button>
    }
    >
      Your Wallet has {btcBalance} BTC. You need an additional {btcAmount - btcBalance} BTC to swap your desired XMR amount.
    </Alert>
  );
}