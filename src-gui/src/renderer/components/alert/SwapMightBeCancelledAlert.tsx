import { makeStyles } from "@material-ui/core";
import { Alert, AlertTitle } from "@material-ui/lab";
import {
  isSwapTimelockInfoCancelled,
  isSwapTimelockInfoNone,
} from "models/rpcModel";
import { useActiveSwapInfo } from "store/hooks";
import HumanizedBitcoinBlockDuration from "../other/HumanizedBitcoinBlockDuration";

const useStyles = makeStyles((theme) => ({
  outer: {
    marginBottom: theme.spacing(1),
  },
  list: {
    margin: theme.spacing(0.25),
  },
}));

export default function SwapMightBeCancelledAlert({
  bobBtcLockTxConfirmations,
}: {
  bobBtcLockTxConfirmations: number;
}) {
  // TODO: Reimplement this using Tauri
  return <></>;

  const classes = useStyles();
  const swap = useActiveSwapInfo();

  if (
    bobBtcLockTxConfirmations < 5 ||
    swap === null ||
    swap.timelock === null
  ) {
    return <></>;
  }

  const { timelock } = swap;
  const punishTimelockOffset = swap.punish_timelock;

  return (
    <Alert severity="warning" className={classes.outer} variant="filled">
      <AlertTitle>Be careful!</AlertTitle>
      The swap provider has taken a long time to lock their Monero. This might
      mean that:
      <ul className={classes.list}>
        <li>
          There is a technical issue that prevents them from locking their funds
        </li>
        <li>They are a malicious actor (unlikely)</li>
      </ul>
      <br />
      There is still hope for the swap to be successful but you have to be extra
      careful. Regardless of why it has taken them so long, it is important that
      you refund the swap within the required time period if the swap is not
      completed. If you fail to to do so, you will be punished and lose your
      money.
      <ul className={classes.list}>
        {isSwapTimelockInfoNone(timelock) && (
          <>
            <li>
              <strong>
                You will be able to refund in about{" "}
                <HumanizedBitcoinBlockDuration
                  blocks={timelock.None.blocks_left}
                />
              </strong>
            </li>

            <li>
              <strong>
                If you have not refunded or completed the swap in about{" "}
                <HumanizedBitcoinBlockDuration
                  blocks={timelock.None.blocks_left + punishTimelockOffset}
                />
                , you will lose your funds.
              </strong>
            </li>
          </>
        )}
        {isSwapTimelockInfoCancelled(timelock) && (
          <li>
            <strong>
              If you have not refunded or completed the swap in about{" "}
              <HumanizedBitcoinBlockDuration
                blocks={timelock.Cancel.blocks_left}
              />
              , you will lose your funds.
            </strong>
          </li>
        )}
        <li>
          As long as you see this screen, the swap will be refunded
          automatically when the time comes. If this fails, you have to manually
          refund by navigating to the History page.
        </li>
      </ul>
    </Alert>
  );
}
