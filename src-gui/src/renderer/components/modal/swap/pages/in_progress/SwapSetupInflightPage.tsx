import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { SatsAmount } from "renderer/components/other/Units";
import CircularProgressWithSubtitle from "../../CircularProgressWithSubtitle";

export default function SwapSetupInflightPage({
  btc_lock_amount,
  btc_tx_lock_fee,
}: TauriSwapProgressEventContent<"SwapSetupInflight">) {
  return (
    <CircularProgressWithSubtitle
      description={
        <>
          Starting swap with provider to lock <SatsAmount amount={btc_lock_amount} />
        </>
      }
    />
  );
}
