import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { formatConfirmations } from "utils/formatUtils";
import BitcoinTransactionInfoBox from "../../BitcoinTransactionInfoBox";
import SwapStatusAlert from "renderer/components/alert/SwapStatusAlert/SwapStatusAlert";
import { useActiveSwapInfo } from "store/hooks";
import { Box, DialogContentText } from "@mui/material";

// This is the number of blocks after which we consider the swap to be at risk of being unsuccessful
const BITCOIN_CONFIRMATIONS_WARNING_THRESHOLD = 2;

export default function BitcoinLockTxInMempoolPage({
  btc_lock_confirmations,
  btc_lock_txid,
}: TauriSwapProgressEventContent<"BtcLockTxInMempool">) {
  const swapInfo = useActiveSwapInfo();

  return (
    <Box>
      {(btc_lock_confirmations === undefined ||
        btc_lock_confirmations < BITCOIN_CONFIRMATIONS_WARNING_THRESHOLD) && (
        <DialogContentText>
          Your Bitcoin has been locked.{" "}
          {btc_lock_confirmations !== undefined && btc_lock_confirmations > 0
            ? "We are waiting for the other party to lock their Monero."
            : "We are waiting for the blockchain to confirm the transaction. Once confirmed, the other party will lock their Monero."}
        </DialogContentText>
      )}
      <Box
        style={{
          display: "flex",
          flexDirection: "column",
          gap: "1rem",
        }}
      >
        {btc_lock_confirmations !== undefined &&
          btc_lock_confirmations >= BITCOIN_CONFIRMATIONS_WARNING_THRESHOLD && (
            <SwapStatusAlert swap={swapInfo} isRunning={true} />
          )}
        <BitcoinTransactionInfoBox
          title="Bitcoin Lock Transaction"
          txId={btc_lock_txid}
          loading
          additionalContent={
            <>
              Most makers require one confirmation before locking their Monero.
              After they lock their funds and the Monero transaction receives
              one confirmation, the swap will proceed to the next step.
              <br />
              Confirmations: {formatConfirmations(btc_lock_confirmations)}
            </>
          }
        />
      </Box>
    </Box>
  );
}
