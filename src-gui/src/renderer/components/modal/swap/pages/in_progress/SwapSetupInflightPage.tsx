import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { SatsAmount, MoneroAmount, PiconeroAmount, MoneroSatsExchangeRate, MoneroBitcoinExchangeRateFromAmounts } from "renderer/components/other/Units";
import CircularProgressWithSubtitle from "../../CircularProgressWithSubtitle";
import {
  Box, Button, Typography, LinearProgress, Divider,
  CircularProgress
} from '@material-ui/core';
import { makeStyles } from '@material-ui/core/styles';
import { ConfirmationRequestPayload } from 'store/features/rpcSlice';
import { useAppSelector } from 'store/hooks';
import PromiseInvokeButton from 'renderer/components/PromiseInvokeButton';

const useStyles = makeStyles((theme) => ({
  confirmationBox: {
    width: '100%',
  },
  timerProgress: {
    marginTop: theme.spacing(1),
    marginBottom: theme.spacing(2),
  },
  actions: {
    marginTop: theme.spacing(2),
    display: 'flex',
    justifyContent: 'center',
    gap: theme.spacing(2),
  },
  valueHighlight: {
    fontWeight: 'bold',
    marginLeft: theme.spacing(1),
  },
  timerContainer: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: theme.spacing(2),
    marginTop: theme.spacing(2),
  },
}));

// Helper to find the relevant confirmation request
const findPreBtcLockRequest = (confirmations: { [key: string]: ConfirmationRequestPayload }): ConfirmationRequestPayload | undefined => {
  return Object.values(confirmations).find(req => req.details.type === 'PreBtcLock');
};

export default function SwapSetupInflightPage({
  btc_lock_amount,
  btc_tx_lock_fee,
}: TauriSwapProgressEventContent<"SwapSetupInflight">) {
  const classes = useStyles();
  const pendingConfirmations = useAppSelector((state) => state.rpc.state.pendingConfirmations);
  const request = findPreBtcLockRequest(pendingConfirmations);

  const [timeLeft, setTimeLeft] = useState<number | null>(null);
  const [progress, setProgress] = useState(100);

  // Timer effect
  useEffect(() => {
    if (request) {
      setTimeLeft(request.timeout_secs);
      setProgress(100);
      const interval = setInterval(() => {
        setTimeLeft((prevTime) => {
          if (prevTime === null || prevTime <= 1) {
            clearInterval(interval);
            return 0;
          }
          const newTime = prevTime - 1;
          if(request) {
             setProgress((newTime / request.timeout_secs) * 100);
          } else {
             setProgress(0); // Or handle error/reset state
             clearInterval(interval);
          }
          return newTime;
        });
      }, 1000);
      return () => clearInterval(interval);
    } else {
      setTimeLeft(null);
      setProgress(100);
    }
  }, [request]);

  if (request) {
    const {btc_lock_amount, btc_network_fee, xmr_receive_amount} = request.details.content;

    return (
        <Box className={classes.confirmationBox}>
           <Typography variant="h6" gutterBottom>Confirm Swap Details</Typography>
           <Divider />
           <Box mt={2} mb={2} textAlign="left">
             <Typography gutterBottom>
                 Please review and confirm the swap amounts below before locking your Bitcoin.
                 <br />
                You lock <SatsAmount amount={btc_lock_amount} />
                <br />
                You pay <SatsAmount amount={btc_network_fee} /> in network fees
                <br />
                You receive <PiconeroAmount amount={xmr_receive_amount} />
             </Typography>
             <Typography>
              Exchange rate: <MoneroBitcoinExchangeRateFromAmounts displayMarkup satsAmount={btc_lock_amount} piconeroAmount={xmr_receive_amount} />
             </Typography>

             {timeLeft !== null && (
               <Box className={classes.timerContainer}>
                 <CircularProgress variant="determinate" value={progress} size={24} />
                 <Typography variant="body2">Time remaining: {timeLeft}s</Typography>
               </Box>
             )}
           </Box>
           <Divider />
           <Box className={classes.actions}>
               <PromiseInvokeButton
                  variant="outlined"
                  disabled={timeLeft === 0 || !request}
                  onInvoke={() => 
                    invoke('deny_confirmation', { requestId: request.request_id })
                  }
                  displayErrorSnackbar={true}
                  requiresContext={true} // Assuming context is needed for the command
               >
                 Deny
               </PromiseInvokeButton>

               <PromiseInvokeButton
                  variant="contained"
                  color="primary"
                  disabled={timeLeft === 0 || !request}
                  onInvoke={ () => 
                     invoke('accept_confirmation', { requestId: request.request_id })
                  }
                  displayErrorSnackbar={true}
                  requiresContext={true} // Assuming context is needed for the command
               >
                 Accept
               </PromiseInvokeButton>
           </Box>
       </Box>
    );
  }

  return (
    <CircularProgressWithSubtitle
      description={
        <>
          Negotiating with maker to swap <SatsAmount amount={btc_lock_amount} />
        </>
      }
    />
  );
}
