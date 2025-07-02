import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  List,
  ListItem,
  ListItemIcon,
  ListItemText,
  Typography,
} from "@mui/material";
import CircleIcon from "@mui/icons-material/Circle";
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
      <DialogTitle>Suspend running swap?</DialogTitle>
      <DialogContent>
        <DialogContentText component="div">
          <List dense>
            <ListItem sx={{ pl: 0 }}>
              <ListItemIcon sx={{ minWidth: "30px" }}>
                <CircleIcon sx={{ fontSize: "8px" }} />
              </ListItemIcon>
              <ListItemText primary="The swap and any network requests between you and the maker will be paused until you resume" />
            </ListItem>
            <ListItem sx={{ pl: 0 }}>
              <ListItemIcon sx={{ minWidth: "30px" }}>
                <CircleIcon sx={{ fontSize: "8px" }} />
              </ListItemIcon>
              <ListItemText
                primary={
                  <>
                    Refund timelocks will <strong>not</strong> be paused. They
                    will continue to count down until they expire
                  </>
                }
              />
            </ListItem>
            <ListItem sx={{ pl: 0 }}>
              <ListItemIcon sx={{ minWidth: "30px" }}>
                <CircleIcon sx={{ fontSize: "8px" }} />
              </ListItemIcon>
              <ListItemText primary="You can monitor the timelock on the history page" />
            </ListItem>
            <ListItem sx={{ pl: 0 }}>
              <ListItemIcon sx={{ minWidth: "30px" }}>
                <CircleIcon sx={{ fontSize: "8px" }} />
              </ListItemIcon>
              <ListItemText primary="If the refund timelock expires, a refund will be initiated in the background. This still requires the app to be running." />
            </ListItem>
          </List>
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
          Suspend
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}
