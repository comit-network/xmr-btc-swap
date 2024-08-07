import { Dialog } from '@material-ui/core';
import { useAppDispatch, useIsRpcEndpointBusy } from 'store/hooks';
import { RpcMethod } from 'models/rpcModel';
import { rpcResetWithdrawTxId } from 'store/features/rpcSlice';
import WithdrawStatePage from './WithdrawStatePage';
import DialogHeader from '../DialogHeader';

export default function WithdrawDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const isRpcEndpointBusy = useIsRpcEndpointBusy(RpcMethod.WITHDRAW_BTC);
  const dispatch = useAppDispatch();

  function onCancel() {
    if (!isRpcEndpointBusy) {
      onClose();
      dispatch(rpcResetWithdrawTxId());
    }
  }

  // This prevents an issue where the Dialog is shown for a split second without a present withdraw state
  if (!open && !isRpcEndpointBusy) return null;

  return (
    <Dialog open onClose={onCancel} maxWidth="sm" fullWidth>
      <DialogHeader title="Withdraw Bitcoin" />
      <WithdrawStatePage onCancel={onCancel} />
    </Dialog>
  );
}
