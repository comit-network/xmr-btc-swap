import { Box } from "@mui/material";
import { Alert } from "@mui/material";
import { useAppSelector } from "store/hooks";
import { SatsAmount } from "../other/Units";
import WalletRefreshButton from "../pages/wallet/WalletRefreshButton";

export default function RemainingFundsWillBeUsedAlert() {
  const balance = useAppSelector((s) => s.rpc.state.balance);

  if (balance == null || balance <= 0) {
    return <></>;
  }

  return (
    <Box sx={{ paddingBottom: 1 }}>
      <Alert
        severity="warning"
        action={<WalletRefreshButton />}
        variant="filled"
      >
        The remaining funds of <SatsAmount amount={balance} /> in the wallet
        will be used for the next swap
      </Alert>
    </Box>
  );
}
