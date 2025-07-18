import { useState, useEffect } from "react";
import {
  DialogTitle,
  DialogContent,
  DialogActions,
  Typography,
  Box,
} from "@mui/material";
import CheckIcon from "@mui/icons-material/Check";
import CloseIcon from "@mui/icons-material/Close";
import { resolveApproval } from "renderer/rpc";
import { usePendingSendMoneroApproval } from "store/hooks";
import { PiconeroAmount } from "renderer/components/other/Units";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";

interface SendApprovalContentProps {
  onClose: () => void;
}

export default function SendApprovalContent({
  onClose,
}: SendApprovalContentProps) {
  const pendingApprovals = usePendingSendMoneroApproval();
  const [timeLeft, setTimeLeft] = useState<number>(0);

  const approval = pendingApprovals[0]; // Handle the first approval request

  useEffect(() => {
    if (
      !approval?.request_status ||
      approval.request_status.state !== "Pending"
    ) {
      return;
    }

    const expirationTs = approval.request_status.content.expiration_ts;
    const expiresAtMs = expirationTs * 1000;

    const tick = () => {
      const remainingMs = Math.max(expiresAtMs - Date.now(), 0);
      setTimeLeft(Math.ceil(remainingMs / 1000));
    };

    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [approval]);

  const handleApprove = async () => {
    if (!approval) throw new Error("No approval request available");
    await resolveApproval(approval.request_id, true);
  };

  const handleReject = async () => {
    if (!approval) throw new Error("No approval request available");
    await resolveApproval(approval.request_id, false);
  };

  if (!approval) {
    return null;
  }

  const { address, amount, fee } = approval.request.content;

  return (
    <>
      <DialogTitle>
        <Typography variant="h6" component="div">
          Confirm Monero Transfer
        </Typography>
      </DialogTitle>

      <DialogContent>
        <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
          {/* Amount */}
          <Box>
            <Typography variant="subtitle2" gutterBottom>
              Amount to Send
            </Typography>
            <Typography variant="h6" color="primary">
              <PiconeroAmount amount={amount} fixedPrecision={12} />
            </Typography>
          </Box>

          {/* Fee */}
          <Box>
            <Typography variant="subtitle2" gutterBottom>
              Network Fee
            </Typography>
            <Typography variant="h6" color="text.secondary">
              <PiconeroAmount amount={fee} fixedPrecision={12} />
            </Typography>
          </Box>

          {/* Destination Address */}
          <Box>
            <Typography variant="subtitle2" gutterBottom>
              Destination Address
            </Typography>
            <Typography variant="h6" color="text.secondary">
              <ActionableMonospaceTextBox
                content={address}
                displayCopyIcon={true}
                enableQrCode={false}
                light={true}
              />
            </Typography>
          </Box>

          {/* Time remaining */}
          <Typography
            variant="caption"
            color="text.secondary"
            sx={{ textAlign: "center" }}
          >
            {timeLeft > 0
              ? `Request expires in ${timeLeft}s`
              : "Request expired"}
          </Typography>
        </Box>
      </DialogContent>

      <DialogActions sx={{ p: 3, gap: 1 }}>
        <PromiseInvokeButton
          onInvoke={handleReject}
          onSuccess={onClose}
          disabled={timeLeft === 0}
          variant="outlined"
          color="error"
          startIcon={<CloseIcon />}
          displayErrorSnackbar={true}
          requiresContext={false}
        >
          Reject
        </PromiseInvokeButton>
        <PromiseInvokeButton
          onInvoke={handleApprove}
          disabled={timeLeft === 0}
          variant="contained"
          color="primary"
          startIcon={<CheckIcon />}
          displayErrorSnackbar={true}
          requiresContext={false}
        >
          Send
        </PromiseInvokeButton>
      </DialogActions>
    </>
  );
}
