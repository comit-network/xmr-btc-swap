import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  TextField,
} from "@material-ui/core";
import { Multiaddr } from "multiaddr";
import { ChangeEvent, useState } from "react";

type ProviderSubmitDialogProps = {
  open: boolean;
  onClose: () => void;
};

export default function ProviderSubmitDialog({
  open,
  onClose,
}: ProviderSubmitDialogProps) {
  const [multiAddr, setMultiAddr] = useState("");
  const [peerId, setPeerId] = useState("");

  async function handleProviderSubmit() {
    if (multiAddr && peerId) {
      await fetch("https://api.unstoppableswap.net/api/submit-provider", {
        method: "post",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          multiAddr,
          peerId,
        }),
      });
      setMultiAddr("");
      setPeerId("");
      onClose();
    }
  }

  function handleMultiAddrChange(event: ChangeEvent<HTMLInputElement>) {
    setMultiAddr(event.target.value);
  }

  function handlePeerIdChange(event: ChangeEvent<HTMLInputElement>) {
    setPeerId(event.target.value);
  }

  function getMultiAddressError(): string | null {
    try {
      const multiAddress = new Multiaddr(multiAddr);
      if (multiAddress.protoNames().includes("p2p")) {
        return "The multi address should not contain the peer id (/p2p/)";
      }
      if (multiAddress.protoNames().find((name) => name.includes("onion"))) {
        return "It is currently not possible to add a provider that is only reachable via Tor";
      }
      return null;
    } catch (e) {
      return "Not a valid multi address";
    }
  }

  return (
    <Dialog onClose={onClose} open={open}>
      <DialogTitle>Submit a provider to the public registry</DialogTitle>
      <DialogContent dividers>
        <DialogContentText>
          If the provider is valid and reachable, it will be displayed to all
          other users to trade with.
        </DialogContentText>
        <TextField
          autoFocus
          margin="dense"
          label="Multiaddress"
          fullWidth
          helperText={
            getMultiAddressError() ||
            "Tells the swap client where the provider can be reached"
          }
          value={multiAddr}
          onChange={handleMultiAddrChange}
          placeholder="/ip4/182.3.21.93/tcp/9939"
          error={!!getMultiAddressError()}
        />
        <TextField
          margin="dense"
          label="Peer ID"
          fullWidth
          helperText="Identifies the provider and allows for secure communication"
          value={peerId}
          onChange={handlePeerIdChange}
          placeholder="12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi"
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <Button
          variant="contained"
          onClick={handleProviderSubmit}
          disabled={!(multiAddr && peerId && !getMultiAddressError())}
          color="primary"
        >
          Submit
        </Button>
      </DialogActions>
    </Dialog>
  );
}
