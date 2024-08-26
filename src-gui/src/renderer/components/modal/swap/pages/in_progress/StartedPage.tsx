import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { SatsAmount } from "renderer/components/other/Units";
import CircularProgressWithSubtitle from "../../CircularProgressWithSubtitle";

export default function StartedPage({
  btc_lock_amount,
  btc_tx_lock_fee,
}: TauriSwapProgressEventContent<"Started">) {
  return (
    <CircularProgressWithSubtitle
      description={
        <>
          Locking <SatsAmount amount={btc_lock_amount} /> with a network fee of{" "}
          <SatsAmount amount={btc_tx_lock_fee} />
        </>
      }
    />
  );
}
