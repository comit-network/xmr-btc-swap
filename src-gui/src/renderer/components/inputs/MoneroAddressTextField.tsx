import {
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  IconButton,
  List,
  ListItemText,
  TextField,
} from "@mui/material";
import { TextFieldProps } from "@mui/material";
import { useEffect, useState } from "react";
import { getMoneroAddresses } from "renderer/rpc";
import { isTestnet } from "store/config";
import { isXmrAddressValid } from "utils/conversionUtils";
import ImportContactsIcon from "@mui/icons-material/ImportContacts";
import TruncatedText from "../other/TruncatedText";

import ListItemButton from "@mui/material/ListItemButton";

type MoneroAddressTextFieldProps = TextFieldProps & {
  address: string;
  onAddressChange: (address: string) => void;
  onAddressValidityChange: (valid: boolean) => void;
  helperText: string;
};

export default function MoneroAddressTextField({
  address,
  onAddressChange,
  onAddressValidityChange,
  helperText,
  ...props
}: MoneroAddressTextFieldProps) {
  const [addresses, setAddresses] = useState<string[]>([]);
  const [showDialog, setShowDialog] = useState(false);

  // Validation
  const placeholder = isTestnet() ? "59McWTPGc745..." : "888tNkZrPN6J...";
  const errorText = isXmrAddressValid(address, isTestnet())
    ? null
    : "Not a valid Monero address";

  // Effects
  useEffect(() => {
    onAddressValidityChange(!errorText);
  }, [address, onAddressValidityChange, errorText]);

  useEffect(() => {
    const fetchAddresses = async () => {
      const response = await getMoneroAddresses();
      setAddresses(response.addresses);
    };
    fetchAddresses();

    const interval = setInterval(fetchAddresses, 5000);
    return () => clearInterval(interval);
  }, []);

  // Event handlers
  const handleClose = () => setShowDialog(false);
  const handleAddressSelect = (selectedAddress: string) => {
    onAddressChange(selectedAddress);
    handleClose();
  };

  return (
    <Box>
      <TextField
        value={address}
        onChange={(e) => onAddressChange(e.target.value)}
        error={!!errorText && address.length > 0}
        helperText={address.length > 0 ? errorText || helperText : helperText}
        placeholder={placeholder}
        variant="outlined"
        slotProps={{
          input: {
            endAdornment: addresses?.length > 0 && (
              <IconButton onClick={() => setShowDialog(true)} size="small">
                <ImportContactsIcon />
              </IconButton>
            ),
          },
        }}
        {...props}
      />

      <RecentlyUsedAddressesDialog
        open={showDialog}
        onClose={handleClose}
        addresses={addresses}
        onAddressSelect={handleAddressSelect}
      />
    </Box>
  );
}

interface RecentlyUsedAddressesDialogProps {
  open: boolean;
  onClose: () => void;
  addresses: string[];
  onAddressSelect: (address: string) => void;
}

function RecentlyUsedAddressesDialog({
  open,
  onClose,
  addresses,
  onAddressSelect,
}: RecentlyUsedAddressesDialogProps) {
  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogContent>
        <List>
          {addresses.map((addr) => (
            <ListItemButton key={addr} onClick={() => onAddressSelect(addr)}>
              <ListItemText
                primary={
                  <Box
                    sx={{
                      fontFamily: "monospace",
                    }}
                  >
                    <TruncatedText limit={40} truncateMiddle>
                      {addr}
                    </TruncatedText>
                  </Box>
                }
                secondary="Recently used as a redeem address"
              />
            </ListItemButton>
          ))}
        </List>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} variant="contained" color="primary">
          Close
        </Button>
      </DialogActions>
    </Dialog>
  );
}
