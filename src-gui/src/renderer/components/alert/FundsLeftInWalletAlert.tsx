import { Button } from "@material-ui/core";
import Alert from "@material-ui/lab/Alert";
import { useNavigate } from "react-router-dom";
import { useAppSelector } from "store/hooks";

export default function FundsLeftInWalletAlert() {
  const fundsLeft = useAppSelector((state) => state.rpc.state.balance);
  const navigate = useNavigate();

  if (fundsLeft != null && fundsLeft > 0) {
    return (
      <Alert
        variant="filled"
        severity="info"
        action={
          <Button
            variant="outlined"
            size="small"
            onClick={() => navigate("/wallet")}
          >
            View
          </Button>
        }
      >
        There are some Bitcoin left in your wallet
      </Alert>
    );
  }
  return null;
}
