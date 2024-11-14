import {
  Box,
  Button,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  makeStyles,
  TextField,
  Theme,
} from "@material-ui/core";
import { ListSellersResponse } from "models/tauriModel";
import { useSnackbar } from "notistack";
import { ChangeEvent, useState } from "react";
import TruncatedText from "renderer/components/other/TruncatedText";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { listSellersAtRendezvousPoint, PRESET_RENDEZVOUS_POINTS } from "renderer/rpc";
import { discoveredProvidersByRendezvous } from "store/features/providersSlice";
import { useAppDispatch } from "store/hooks";
import { isValidMultiAddressWithPeerId } from "utils/parseUtils";

const useStyles = makeStyles((theme: Theme) => ({
  chipOuter: {
    display: "flex",
    flexWrap: "wrap",
    gap: theme.spacing(1),
  },
}));

type ListSellersDialogProps = {
  open: boolean;
  onClose: () => void;
};

export default function ListSellersDialog({
  open,
  onClose,
}: ListSellersDialogProps) {
  const classes = useStyles();
  const [rendezvousAddress, setRendezvousAddress] = useState("");
  const { enqueueSnackbar } = useSnackbar();
  const dispatch = useAppDispatch();

  function handleMultiAddrChange(event: ChangeEvent<HTMLInputElement>) {
    setRendezvousAddress(event.target.value);
  }

  function getMultiAddressError(): string | null {
    return isValidMultiAddressWithPeerId(rendezvousAddress)
      ? null
      : "Address is invalid or missing peer ID";
  }

  function handleSuccess({ sellers }: ListSellersResponse) {
    dispatch(discoveredProvidersByRendezvous(sellers));

    const discoveredSellersCount = sellers.length;
    let message: string;

    switch (discoveredSellersCount) {
      case 0:
        message = `No providers were discovered at the rendezvous point`;
        break;
      case 1:
        message = `Discovered one provider at the rendezvous point`;
        break;
      default:
        message = `Discovered ${discoveredSellersCount} providers at the rendezvous point`;
    }

    enqueueSnackbar(message, {
      variant: "success",
      autoHideDuration: 5000,
    });

    onClose();
  }

  return (
    <Dialog onClose={onClose} open={open}>
      <DialogTitle>Discover swap providers</DialogTitle>
      <DialogContent dividers>
        <DialogContentText>
          The rendezvous protocol provides a way to discover providers (trading
          partners) without relying on one singular centralized institution. By
          manually connecting to a rendezvous point run by a volunteer, you can
          discover providers and then connect and swap with them.
        </DialogContentText>
        <TextField
          autoFocus
          margin="dense"
          label="Rendezvous point"
          fullWidth
          helperText={
            getMultiAddressError() || "Multiaddress of the rendezvous point"
          }
          value={rendezvousAddress}
          onChange={handleMultiAddrChange}
          placeholder="/dns4/discover.unstoppableswap.net/tcp/8888/p2p/12D3KooWA6cnqJpVnreBVnoro8midDL9Lpzmg8oJPoAGi7YYaamE"
          error={!!getMultiAddressError()}
        />
        <Box className={classes.chipOuter}>
          {PRESET_RENDEZVOUS_POINTS.map((rAddress) => (
            <Chip
              key={rAddress}
              clickable
              label={<TruncatedText limit={30}>{rAddress}</TruncatedText>}
              onClick={() => setRendezvousAddress(rAddress)}
            />
          ))}
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <PromiseInvokeButton
          variant="contained"
          disabled={
            // We disable the button if the multiaddress is invalid
            getMultiAddressError() !== null
          }
          color="primary"
          onSuccess={handleSuccess}
          onInvoke={() => listSellersAtRendezvousPoint(rendezvousAddress)}
        >
          Connect
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}
