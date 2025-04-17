import { useState, useEffect } from 'react';
import { resolveApproval } from 'renderer/rpc';
import { PendingLockBitcoinApprovalRequest, TauriSwapProgressEventContent } from 'models/tauriModelExt';
import {
  SatsAmount,
  PiconeroAmount,
  MoneroBitcoinExchangeRateFromAmounts
} from 'renderer/components/other/Units';
import {
  Box,
  Typography,
  Divider,
} from '@material-ui/core';
import { makeStyles, createStyles, Theme } from '@material-ui/core/styles';
import { useActiveSwapId, usePendingLockBitcoinApproval } from 'store/hooks';
import PromiseInvokeButton from 'renderer/components/PromiseInvokeButton';
import InfoBox from 'renderer/components/modal/swap/InfoBox';
import CircularProgressWithSubtitle from '../../CircularProgressWithSubtitle';
import CheckIcon from '@material-ui/icons/Check';

const useStyles = makeStyles((theme: Theme) =>
  createStyles({
    detailGrid: {
      display: 'grid',
      gridTemplateColumns: 'auto 1fr',
      rowGap: theme.spacing(1),
      columnGap: theme.spacing(2),
      alignItems: 'center',
      marginBlock: theme.spacing(2),
    },
    label: {
      color: theme.palette.text.secondary,
    },
    receiveValue: {
      fontWeight: 'bold',
      color: theme.palette.success.main,
    },
    actions: {
      marginTop: theme.spacing(2),
      display: 'flex',
      justifyContent: 'flex-end',
      gap: theme.spacing(2),
    },
    cancelButton: {
      color: theme.palette.text.secondary,
    },
  })
);

/// A hook that returns the LockBitcoin confirmation request for the active swap
/// Returns null if no confirmation request is found
function useActiveLockBitcoinApprovalRequest(): PendingLockBitcoinApprovalRequest | null {
  const approvals = usePendingLockBitcoinApproval();
  const activeSwapId = useActiveSwapId();

  return approvals
    ?.find(r => r.content.details.content.swap_id === activeSwapId) || null;
}

export default function SwapSetupInflightPage({
  btc_lock_amount,
}: TauriSwapProgressEventContent<'SwapSetupInflight'>) {
  const classes = useStyles();
  const request = useActiveLockBitcoinApprovalRequest();

  const [timeLeft, setTimeLeft] = useState<number>(0);

  const expiresAtMs = request?.content.expiration_ts * 1000 || 0;

  useEffect(() => {
    const tick = () => {
      const remainingMs = Math.max(expiresAtMs - Date.now(), 0);
      setTimeLeft(Math.ceil(remainingMs / 1000));
    };

    tick();
    const id = setInterval(tick, 250);
    return () => clearInterval(id);
  }, [expiresAtMs]);

  // If we do not have an approval request yet for the Bitcoin lock transaction, we haven't received the offer from Alice yet
  // Display a loading spinner to the user for as long as the swap_setup request is in flight
  if (!request) {
    return <CircularProgressWithSubtitle description={<>Negotiating offer for <SatsAmount amount={btc_lock_amount} /></>} />;
  }

  const { btc_network_fee, xmr_receive_amount } = request.content.details.content;

  return (
    <InfoBox
      title="Approve Swap"
      icon={<></>}
      loading={false}
      mainContent={
        <>
          <Divider />
          <Box className={classes.detailGrid}>
            <Typography className={classes.label}>You send</Typography>
            <Typography>
              <SatsAmount amount={btc_lock_amount} />
            </Typography>

            <Typography className={classes.label}>Bitcoin network fees</Typography>
            <Typography>
              <SatsAmount amount={btc_network_fee} />
            </Typography>

            <Typography className={classes.label}>You receive</Typography>
            <Typography className={classes.receiveValue}>
              <PiconeroAmount amount={xmr_receive_amount} />
            </Typography>

            <Typography className={classes.label}>Exchange rate</Typography>
            <Typography>
              <MoneroBitcoinExchangeRateFromAmounts
                satsAmount={btc_lock_amount}
                piconeroAmount={xmr_receive_amount}
                displayMarkup
              />
            </Typography>
          </Box>
        </>
      }
      additionalContent={
        <Box className={classes.actions}>
          <PromiseInvokeButton
            variant="text"
            size="large"
            className={classes.cancelButton}
            onInvoke={() => resolveApproval(request.content.request_id, false)}
            displayErrorSnackbar
            requiresContext
          >
            Deny
          </PromiseInvokeButton>

          <PromiseInvokeButton
            variant="contained"
            color="primary"
            size="large"
            onInvoke={() => resolveApproval(request.content.request_id, true)}
            displayErrorSnackbar
            requiresContext
            endIcon={<CheckIcon />}
          >
            {`Confirm & lock BTC (${timeLeft}s)`}
          </PromiseInvokeButton>
        </Box>
      }
    />
  );
}