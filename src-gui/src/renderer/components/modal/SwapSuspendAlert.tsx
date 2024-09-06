import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
} from "@material-ui/core";
import { suspendCurrentSwap } from "renderer/rpc";
import PromiseInvokeButton from "../PromiseInvokeButton";

type SwapCancelAlertProps = {
  open: boolean;
  onClose: () => void;
};

export default function SwapSuspendAlert({
  open,
  onClose,
}: SwapCancelAlertProps) {
  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>Force stop running operation?</DialogTitle>
      <DialogContent>
        <DialogContentText>
          Are you sure you want to force stop the running swap?
        </DialogContentText>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} color="primary">
          No
        </Button>
        <PromiseInvokeButton
          color="primary"
          onSuccess={onClose}
          onInvoke={suspendCurrentSwap}
        >
          Force stop
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}
