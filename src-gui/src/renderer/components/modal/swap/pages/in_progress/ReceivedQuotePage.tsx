import {
  useConservativeBitcoinSyncProgress,
  usePendingBackgroundProcesses,
} from "store/hooks";
import CircularProgressWithSubtitle, {
  LinearProgressWithSubtitle,
} from "../../CircularProgressWithSubtitle";

export default function ReceivedQuotePage() {
  const syncProgress = useConservativeBitcoinSyncProgress();

  if (syncProgress?.type === "Known") {
    const percentage = Math.round(
      (syncProgress.content.consumed / syncProgress.content.total) * 100,
    );

    return (
      <LinearProgressWithSubtitle
        description={`Syncing Bitcoin wallet`}
        value={percentage}
      />
    );
  }

  if (syncProgress?.type === "Unknown") {
    return (
      <CircularProgressWithSubtitle description="Syncing Bitcoin wallet" />
    );
  }

  return <CircularProgressWithSubtitle description="Processing offer" />;
}
