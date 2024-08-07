import { Box, Button, makeStyles, Typography } from '@material-ui/core';
import { useState } from 'react';
import SendIcon from '@material-ui/icons/Send';
import { useAppSelector, useIsRpcEndpointBusy } from 'store/hooks';
import { RpcMethod } from 'models/rpcModel';
import BitcoinIcon from '../../icons/BitcoinIcon';
import WithdrawDialog from '../../modal/wallet/WithdrawDialog';
import WalletRefreshButton from './WalletRefreshButton';
import InfoBox from '../../modal/swap/InfoBox';
import { SatsAmount } from 'renderer/components/other/Units';

const useStyles = makeStyles((theme) => ({
  title: {
    alignItems: 'center',
    display: 'flex',
    gap: theme.spacing(0.5),
  },
}));

export default function WithdrawWidget() {
  const classes = useStyles();
  const walletBalance = useAppSelector((state) => state.rpc.state.balance);
  const checkingBalance = useIsRpcEndpointBusy(RpcMethod.GET_BTC_BALANCE);
  const [showDialog, setShowDialog] = useState(false);

  function onShowDialog() {
    setShowDialog(true);
  }

  return (
    <>
      <InfoBox
        title={
          <Box className={classes.title}>
            Wallet Balance
            <WalletRefreshButton />
          </Box>
        }
        mainContent={
          <Typography variant="h5">
            <SatsAmount amount={walletBalance} />
          </Typography>
        }
        icon={<BitcoinIcon />}
        additionalContent={
          <Button
            variant="contained"
            color="primary"
            endIcon={<SendIcon />}
            size="large"
            onClick={onShowDialog}
            disabled={
              walletBalance === null || checkingBalance || walletBalance <= 0
            }
          >
            Withdraw
          </Button>
        }
        loading={false}
      />
      <WithdrawDialog open={showDialog} onClose={() => setShowDialog(false)} />
    </>
  );
}
