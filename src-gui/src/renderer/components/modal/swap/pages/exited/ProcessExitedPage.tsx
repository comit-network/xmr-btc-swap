import { TauriSwapProgressEvent } from "models/tauriModel";
import SwapStatePage from "../SwapStatePage";

export default function ProcessExitedPage({
  prevState,
  swapId,
}: {
  prevState: TauriSwapProgressEvent | null;
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

  // TODO: Display something useful here
  return (
    <>
      If the swap is not a "done" state (or we don't have a db state because the
      swap did complete the SwapSetup yet) we should tell the user and show logs
      Not implemented yet
    </>
  );

  // If the swap is not a "done" state (or we don't have a db state because the swap did complete the SwapSetup yet) we should tell the user and show logs
  // return <ProcessExitedAndNotDonePage state={state} />;
}
