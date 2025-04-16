import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useAppSelector } from '../../../store/hooks'; // Adjust path
import { ConfirmationRequestPayload } from '../../../store/features/rpcSlice'; // Adjust path
import {
  Dialog, DialogTitle, DialogContent, DialogActions, Button, Typography, Box, LinearProgress
} from '@material-ui/core';
import { makeStyles } from '@material-ui/core/styles';

const useStyles = makeStyles((theme) => ({
  content: {
    minWidth: '400px',
    paddingTop: theme.spacing(2),
    paddingBottom: theme.spacing(2),
  },
  jsonBox: {
    maxHeight: '200px',
    overflowY: 'auto',
    backgroundColor: theme.palette.grey[100],
    padding: theme.spacing(1),
    marginTop: theme.spacing(2),
    marginBottom: theme.spacing(2),
    border: `1px solid ${theme.palette.grey[300]}`,
    borderRadius: theme.shape.borderRadius,
    whiteSpace: 'pre-wrap', // Ensure JSON formatting is preserved
    wordBreak: 'break-all',
  },
  timerProgress: {
    marginTop: theme.spacing(2),
  },
}));

function ConfirmationModal() {
  const classes = useStyles();
  const pendingConfirmations = useAppSelector((state) => state.rpc.state.pendingConfirmations);
  const request: ConfirmationRequestPayload | undefined = Object.values(pendingConfirmations)[0];

  const [timeLeft, setTimeLeft] = useState<number | null>(null);
  const [progress, setProgress] = useState(100);

  useEffect(() => {
    if (request) {
      setTimeLeft(request.timeout_secs);
      setProgress(100);
      const interval = setInterval(() => {
        setTimeLeft((prevTime) => {
          if (prevTime === null || prevTime <= 1) {
            clearInterval(interval);
            // Timeout likely handled by backend sending resolved event, but we can clear UI state too
            return 0;
          }
          const newTime = prevTime - 1;
          setProgress((newTime / request.timeout_secs) * 100);
          return newTime;
        });
      }, 1000);
      return () => clearInterval(interval); // Cleanup interval on unmount or request change
    } else {
      setTimeLeft(null); // Reset timer if no request
      setProgress(100);
    }
  }, [request]); // Rerun effect when request changes

  const handleAccept = async () => {
    if (!request) return;
    try {
      await invoke('accept_confirmation', { requestId: request.request_id });
    } catch (error) {
      console.error("Failed to accept confirmation:", error);
      // TODO: Display error to user (e.g., using a snackbar)
    }
    // Modal will close automatically via event listener and state update
  };

  const handleDeny = async () => {
    if (!request) return;
    try {
      await invoke('deny_confirmation', { requestId: request.request_id });
    } catch (error) {
      console.error("Failed to deny confirmation:", error);
      // TODO: Display error to user
    }
    // Modal will close automatically via event listener and state update
  };

  if (!request) {
    return null; // Don't render anything if no request is pending
  }

  // Parse state2_json for display
  let parsedDetails: Record<string, unknown> | string = 'Invalid swap details';
  if (request.type === 'PreBtcLock') {
      try {
          parsedDetails = JSON.parse(request.state2_json);
      } catch (e) {
          console.error("Failed to parse state2_json:", e);
          parsedDetails = request.state2_json; // Show raw string on error
      }
  }

  return (
    <Dialog open={!!request} aria-labelledby="confirmation-dialog-title">
      <DialogTitle id="confirmation-dialog-title">Confirm Action</DialogTitle>
      <DialogContent dividers className={classes.content}>
        <Typography gutterBottom>
Please review the details below and confirm to proceed.
        </Typography>

        {request.type === 'PreBtcLock' && (
            <> 
                <Typography variant="h6" gutterBottom>Swap Details (Pre-Lock):</Typography>
                {/* Improve rendering based on actual state2 structure */} 
                <Box className={classes.jsonBox}>
                    <pre>{JSON.stringify(parsedDetails, null, 2)}</pre>
                </Box>
            </>
        )}
        
        {/* Add sections for other ConfirmationRequestType variants here if needed */} 

        {timeLeft !== null && (
          <Box mt={2}>
            <Typography variant="body2" align="center">Time remaining: {timeLeft}s</Typography>
            <LinearProgress variant="determinate" value={progress} className={classes.timerProgress} />
          </Box>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={handleDeny} color="secondary" disabled={timeLeft === 0}>
          Deny
        </Button>
        <Button onClick={handleAccept} color="primary" variant="contained" disabled={timeLeft === 0}>
          Accept
        </Button>
      </DialogActions>
    </Dialog>
  );
}

export default ConfirmationModal; 