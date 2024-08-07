import { useAppSelector, useIsRpcEndpointBusy } from 'store/hooks';
import { RpcMethod } from 'models/rpcModel';
import AddressInputPage from './pages/AddressInputPage';
import InitiatedPage from './pages/InitiatedPage';
import BtcTxInMempoolPageContent from './pages/BitcoinWithdrawTxInMempoolPage';

export default function WithdrawStatePage({
  onCancel,
}: {
  onCancel: () => void;
}) {
  const isRpcEndpointBusy = useIsRpcEndpointBusy(RpcMethod.WITHDRAW_BTC);
  const withdrawTxId = useAppSelector((state) => state.rpc.state.withdrawTxId);

  if (withdrawTxId !== null) {
    return (
      <BtcTxInMempoolPageContent
        withdrawTxId={withdrawTxId}
        onCancel={onCancel}
      />
    );
  }
  if (isRpcEndpointBusy) {
    return <InitiatedPage onCancel={onCancel} />;
  }
  return <AddressInputPage onCancel={onCancel} />;
}
