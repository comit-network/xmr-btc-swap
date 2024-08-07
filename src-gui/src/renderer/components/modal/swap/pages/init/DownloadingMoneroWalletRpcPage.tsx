import CircularProgressWithSubtitle from '../../CircularProgressWithSubtitle';
import { MoneroWalletRpcUpdateState } from '../../../../../../models/storeModel';

export default function DownloadingMoneroWalletRpcPage({
  updateState,
}: {
  updateState: MoneroWalletRpcUpdateState;
}) {
  return (
    <CircularProgressWithSubtitle
      description={`Updating monero-wallet-rpc (${updateState.progress}) `}
    />
  );
}
