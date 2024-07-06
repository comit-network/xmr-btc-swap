import { CircularProgress } from '@material-ui/core';
import RefreshIcon from '@material-ui/icons/Refresh';
import IpcInvokeButton from '../../IpcInvokeButton';

export default function WalletRefreshButton() {
  return (
    <IpcInvokeButton
      loadIcon={<CircularProgress size={24} />}
      size="small"
      isIconButton
      endIcon={<RefreshIcon />}
      ipcArgs={[]}
      ipcChannel="spawn-balance-check"
    />
  );
}
