import { Button, CircularProgress, IconButton } from '@material-ui/core';
import RefreshIcon from '@material-ui/icons/Refresh';
import IpcInvokeButton from '../../IpcInvokeButton';
import { checkBitcoinBalance } from 'renderer/rpc';

export default function WalletRefreshButton() {
  return <IconButton onClick={() => checkBitcoinBalance(true)}>
    <RefreshIcon />
  </IconButton>
}
