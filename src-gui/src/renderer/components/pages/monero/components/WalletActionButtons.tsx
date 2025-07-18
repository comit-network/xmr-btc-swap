import {
  Box,
  Button,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  IconButton,
  ListItemIcon,
  Menu,
  MenuItem,
  TextField,
  Typography,
} from "@mui/material";
import {
  Send as SendIcon,
  SwapHoriz as SwapIcon,
  Restore as RestoreIcon,
  MoreHoriz as MoreHorizIcon,
} from "@mui/icons-material";
import { useState } from "react";
import { setMoneroRestoreHeight } from "renderer/rpc";
import SendTransactionModal from "../SendTransactionModal";
import { useNavigate } from "react-router-dom";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import SetRestoreHeightModal from "../SetRestoreHeightModal";

interface WalletActionButtonsProps {
  balance: {
    unlocked_balance: string;
  };
}

function RestoreHeightDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [restoreHeight, setRestoreHeight] = useState(0);

  const handleRestoreHeight = async () => {
    await setMoneroRestoreHeight(restoreHeight);
    onClose();
  };

  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>Restore Height</DialogTitle>
      <DialogContent>
        <TextField
          label="Restore Height"
          type="number"
          value={restoreHeight}
          onChange={(e) => setRestoreHeight(Number(e.target.value))}
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <PromiseInvokeButton
          onInvoke={handleRestoreHeight}
          displayErrorSnackbar={true}
          variant="contained"
        >
          Restore
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}

export default function WalletActionButtons({
  balance,
}: WalletActionButtonsProps) {
  const navigate = useNavigate();
  const [sendDialogOpen, setSendDialogOpen] = useState(false);
  const [restoreHeightDialogOpen, setRestoreHeightDialogOpen] = useState(false);

  const [menuAnchorEl, setMenuAnchorEl] = useState<null | HTMLElement>(null);
  const menuOpen = Boolean(menuAnchorEl);
  const handleMenuClick = (event: React.MouseEvent<HTMLButtonElement>) => {
    setMenuAnchorEl(event.currentTarget);
  };
  const handleMenuClose = () => {
    setMenuAnchorEl(null);
  };

  return (
    <>
      <SetRestoreHeightModal
        open={restoreHeightDialogOpen}
        onClose={() => setRestoreHeightDialogOpen(false)}
      />
      <SendTransactionModal
        balance={balance}
        open={sendDialogOpen}
        onClose={() => setSendDialogOpen(false)}
      />
      <Box
        sx={{
          display: "flex",
          flexWrap: "wrap",
          gap: 1,
          mb: 2,
          alignItems: "center",
        }}
      >
        <Chip
          icon={<SendIcon />}
          label="Send"
          variant="button"
          clickable
          onClick={() => setSendDialogOpen(true)}
        />
        <Chip
          onClick={() => navigate("/swap")}
          icon={<SwapIcon />}
          label="Swap"
          variant="button"
          clickable
        />

        <IconButton onClick={handleMenuClick}>
          <MoreHorizIcon />
        </IconButton>
        <Menu anchorEl={menuAnchorEl} open={menuOpen} onClose={handleMenuClose}>
          <MenuItem
            onClick={() => {
              setRestoreHeightDialogOpen(true);
              handleMenuClose();
            }}
          >
            <ListItemIcon>
              <RestoreIcon />
            </ListItemIcon>
            <Typography>Restore Height</Typography>
          </MenuItem>
        </Menu>
      </Box>
    </>
  );
}
