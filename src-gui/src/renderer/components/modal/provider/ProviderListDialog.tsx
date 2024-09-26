import {
  Avatar,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  List,
  ListItem,
  ListItemAvatar,
  ListItemText,
  makeStyles,
} from "@material-ui/core";
import AddIcon from "@material-ui/icons/Add";
import SearchIcon from "@material-ui/icons/Search";
import { ExtendedProviderStatus } from "models/apiModel";
import { useState } from "react";
import { setSelectedProvider } from "store/features/providersSlice";
import { useAllProviders, useAppDispatch } from "store/hooks";
import ListSellersDialog from "../listSellers/ListSellersDialog";
import ProviderInfo from "./ProviderInfo";
import ProviderSubmitDialog from "./ProviderSubmitDialog";

const useStyles = makeStyles({
  dialogContent: {
    padding: 0,
  },
});

type ProviderSelectDialogProps = {
  open: boolean;
  onClose: () => void;
};

export function ProviderSubmitDialogOpenButton() {
  const [open, setOpen] = useState(false);

  return (
    <ListItem
      autoFocus
      button
      onClick={() => {
        // Prevents background from being clicked and reopening dialog
        if (!open) {
          setOpen(true);
        }
      }}
    >
      <ProviderSubmitDialog open={open} onClose={() => setOpen(false)} />
      <ListItemAvatar>
        <Avatar>
          <AddIcon />
        </Avatar>
      </ListItemAvatar>
      <ListItemText primary="Add a new provider to public registry" />
    </ListItem>
  );
}

export function ListSellersDialogOpenButton() {
  const [open, setOpen] = useState(false);

  return (
    <ListItem
      autoFocus
      button
      onClick={() => {
        // Prevents background from being clicked and reopening dialog
        if (!open) {
          setOpen(true);
        }
      }}
    >
      <ListSellersDialog open={open} onClose={() => setOpen(false)} />
      <ListItemAvatar>
        <Avatar>
          <SearchIcon />
        </Avatar>
      </ListItemAvatar>
      <ListItemText primary="Discover providers by connecting to a rendezvous point" />
    </ListItem>
  );
}

export default function ProviderListDialog({
  open,
  onClose,
}: ProviderSelectDialogProps) {
  const classes = useStyles();
  const providers = useAllProviders();
  const dispatch = useAppDispatch();

  function handleProviderChange(provider: ExtendedProviderStatus) {
    dispatch(setSelectedProvider(provider));
    onClose();
  }

  return (
    <Dialog onClose={onClose} open={open}>
      <DialogTitle>Select a swap provider</DialogTitle>

      <DialogContent className={classes.dialogContent} dividers>
        <List>
          {providers.map((provider) => (
            <ListItem
              button
              onClick={() => handleProviderChange(provider)}
              key={provider.peerId}
            >
              <ProviderInfo provider={provider} key={provider.peerId} />
            </ListItem>
          ))}
          <ListSellersDialogOpenButton />
          <ProviderSubmitDialogOpenButton />
        </List>
      </DialogContent>

      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
      </DialogActions>
    </Dialog>
  );
}
