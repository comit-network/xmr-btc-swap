import { MoneroWalletRpcUpdateState } from "../../../../../../models/storeModel";
import CircularProgressWithSubtitle from "../../CircularProgressWithSubtitle";

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
