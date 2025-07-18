import {
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  TextField,
  Button,
  IconButton,
  InputAdornment,
} from "@mui/material";
import { Visibility, VisibilityOff } from "@mui/icons-material";
import { useState } from "react";
import { usePendingPasswordApproval } from "store/hooks";
import { rejectApproval, resolveApproval } from "renderer/rpc";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";

export default function PasswordEntryDialog() {
  const pendingApprovals = usePendingPasswordApproval();
  const [password, setPassword] = useState<string>("");
  const [error, setError] = useState<string>("");
  const [showPassword, setShowPassword] = useState<boolean>(false);

  const approval = pendingApprovals[0];

  const accept = async () => {
    if (!approval) {
      throw new Error("No approval request found for password entry");
    }

    try {
      await resolveApproval<string>(approval.request_id, password);
      setPassword("");
      setError("");
    } catch (err) {
      setError("Invalid password. Please try again.");
      throw err;
    }
  };

  const reject = async () => {
    if (!approval) {
      throw new Error("No approval request found for password entry");
    }

    try {
      await rejectApproval<string>(approval.request_id, "");
      setPassword("");
      setError("");
    } catch (err) {
      console.error("Error rejecting password request:", err);
      throw err;
    }
  };

  const handleTogglePasswordVisibility = () => {
    setShowPassword(!showPassword);
  };

  if (!approval) {
    return null;
  }

  return (
    <Dialog
      open={true}
      maxWidth="sm"
      fullWidth
      BackdropProps={{
        sx: {
          backdropFilter: "blur(8px)",
          backgroundColor: "rgba(0, 0, 0, 0.5)",
        },
      }}
    >
      <DialogTitle>Enter Wallet Password</DialogTitle>

      <DialogContent>
        <TextField
          fullWidth
          type={showPassword ? "text" : "password"}
          label="Password"
          value={password}
          onChange={(e) => {
            setPassword(e.target.value);
            if (error) setError("");
          }}
          error={!!error}
          helperText={error}
          autoFocus
          margin="normal"
          onKeyPress={(e) => {
            if (e.key === "Enter") {
              accept();
            }
          }}
          InputProps={{
            endAdornment: (
              <InputAdornment position="end">
                <IconButton
                  onClick={handleTogglePasswordVisibility}
                  edge="end"
                  aria-label="toggle password visibility"
                >
                  {showPassword ? <VisibilityOff /> : <Visibility />}
                </IconButton>
              </InputAdornment>
            ),
          }}
        />
      </DialogContent>

      <DialogActions>
        <Button onClick={reject} variant="outlined">
          Change wallet
        </Button>
        <PromiseInvokeButton
          onInvoke={accept}
          variant="contained"
          requiresContext={false}
        >
          Unlock
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}
