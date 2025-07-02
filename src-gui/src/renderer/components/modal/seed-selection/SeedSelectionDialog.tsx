import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControl,
  FormControlLabel,
  Radio,
  RadioGroup,
  TextField,
  Typography,
} from "@mui/material";
import { useState, useEffect } from "react";
import { usePendingSeedSelectionApproval } from "store/hooks";
import { resolveApproval, checkSeed } from "renderer/rpc";

export default function SeedSelectionDialog() {
  const pendingApprovals = usePendingSeedSelectionApproval();
  const [selectedOption, setSelectedOption] = useState<string>("RandomSeed");
  const [customSeed, setCustomSeed] = useState<string>("");
  const [isSeedValid, setIsSeedValid] = useState<boolean>(false);
  const approval = pendingApprovals[0]; // Handle the first pending approval

  useEffect(() => {
    if (selectedOption === "FromSeed" && customSeed.trim()) {
      checkSeed(customSeed.trim())
        .then((valid) => {
          setIsSeedValid(valid);
        })
        .catch(() => {
          setIsSeedValid(false);
        });
    } else {
      setIsSeedValid(false);
    }
  }, [customSeed, selectedOption]);

  const handleClose = async (accept: boolean) => {
    if (!approval) return;

    if (accept) {
      const seedChoice =
        selectedOption === "RandomSeed"
          ? { type: "RandomSeed" }
          : { type: "FromSeed", content: { seed: customSeed } };

      await resolveApproval(approval.request_id, seedChoice);
    } else {
      // On reject, just close without approval
      await resolveApproval(approval.request_id, { type: "RandomSeed" });
    }
  };

  if (!approval) {
    return null;
  }

  return (
    <Dialog open={true} maxWidth="sm" fullWidth>
      <DialogTitle>Monero Wallet</DialogTitle>
      <DialogContent>
        <Typography variant="body1" sx={{ mb: 2 }}>
          Choose what seed to use for the wallet.
        </Typography>

        <FormControl component="fieldset">
          <RadioGroup
            value={selectedOption}
            onChange={(e) => setSelectedOption(e.target.value)}
          >
            <FormControlLabel
              value="RandomSeed"
              control={<Radio />}
              label="Create a new wallet"
            />
            <FormControlLabel
              value="FromSeed"
              control={<Radio />}
              label="Restore wallet from seed"
            />
          </RadioGroup>
        </FormControl>

        {selectedOption === "FromSeed" && (
          <TextField
            fullWidth
            multiline
            rows={3}
            label="Enter your seed phrase"
            value={customSeed}
            onChange={(e) => setCustomSeed(e.target.value)}
            sx={{ mt: 2 }}
            placeholder="Enter your Monero 25 words seed phrase..."
            error={!isSeedValid && customSeed.length > 0}
            helperText={
              isSeedValid
                ? "Seed is valid"
                : customSeed.length > 0
                  ? "Seed is invalid"
                  : ""
            }
          />
        )}
      </DialogContent>
      <DialogActions>
        <Button
          onClick={() => handleClose(true)}
          variant="contained"
          disabled={
            selectedOption === "FromSeed"
              ? !customSeed.trim() || !isSeedValid
              : false
          }
        >
          Confirm
        </Button>
      </DialogActions>
    </Dialog>
  );
}
