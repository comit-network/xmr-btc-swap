import { TauriSwapProgressEvent } from "models/tauriModel";
import { TauriSwapProgressEventType } from "models/tauriModelExt";
import SwapStatePage from "../SwapStatePage";
import ProcessExitedAndNotDonePage from "./ProcessExitedAndNotDonePage";

export default function ProcessExitedPage({
  prevState,
  currState,
  swapId,
}: {
  prevState: TauriSwapProgressEvent | null;
  currState: TauriSwapProgressEventType<"Released">;
  swapId: string;
}) {
  // If we have a previous state, we can show the user the last state of the swap
  // We only show the last state if its a final state (XmrRedeemInMempool, BtcRefunded, BtcPunished)
  if (
    prevState != null &&
    (prevState.type === "XmrRedeemInMempool" ||
      prevState.type === "BtcRefunded" ||
      prevState.type === "BtcPunished")
  ) {
    return (
      <SwapStatePage
        state={{
          curr: prevState,
          prev: null,
          swapId,
        }}
      />
    );
  }

  return <ProcessExitedAndNotDonePage currState={currState.content} />;
}
