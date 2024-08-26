import { Box, makeStyles, Typography } from "@material-ui/core";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { useAppSelector } from "store/hooks";
import BitcoinIcon from "../../../../icons/BitcoinIcon";
import { MoneroSatsExchangeRate, SatsAmount } from "../../../../other/Units";
import DepositAddressInfoBox from "../../DepositAddressInfoBox";
import DepositAmountHelper from "./DepositAmountHelper";

const useStyles = makeStyles((theme) => ({
  amountHelper: {
    display: "flex",
    alignItems: "center",
  },
  additionalContent: {
    paddingTop: theme.spacing(1),
    gap: theme.spacing(0.5),
    display: "flex",
    flexDirection: "column",
  },
}));

export default function WaitingForBtcDepositPage({
  deposit_address,
  min_deposit_until_swap_will_start,
  max_deposit_until_maximum_amount_is_reached,
  min_bitcoin_lock_tx_fee,
  quote,
}: TauriSwapProgressEventContent<"WaitingForBtcDeposit">) {
  const classes = useStyles();
  const bitcoinBalance = useAppSelector((s) => s.rpc.state.balance) || 0;

  // TODO: Account for BTC lock tx fees
  return (
    <Box>
      <DepositAddressInfoBox
        title="Bitcoin Deposit Address"
        address={deposit_address}
        additionalContent={
          <Box className={classes.additionalContent}>
            <Typography variant="subtitle2">
              <ul>
                {bitcoinBalance > 0 ? (
                  <li>
                    You have already deposited{" "}
                    <SatsAmount amount={bitcoinBalance} />
                  </li>
                ) : null}
                <li>
                  Send any amount between{" "}
                  <SatsAmount amount={min_deposit_until_swap_will_start} /> and{" "}
                  <SatsAmount
                    amount={max_deposit_until_maximum_amount_is_reached}
                  />{" "}
                  to the address above
                  {bitcoinBalance > 0 && (
                    <> (on top of the already deposited funds)</>
                  )}
                </li>
                <li>
                  All Bitcoin sent to this this address will converted into
                  Monero at an exchance rate of{" "}
                  <MoneroSatsExchangeRate rate={quote.price} />
                </li>
                <li>
                  The network fee of{" "}
                  <SatsAmount amount={min_bitcoin_lock_tx_fee} /> will
                  automatically be deducted from the deposited coins
                </li>
                <li>
                  The swap will start automatically as soon as the minimum
                  amount is deposited
                </li>
              </ul>
            </Typography>
            <DepositAmountHelper
              min_deposit_until_swap_will_start={
                min_deposit_until_swap_will_start
              }
              max_deposit_until_maximum_amount_is_reached={
                max_deposit_until_maximum_amount_is_reached
              }
              min_bitcoin_lock_tx_fee={min_bitcoin_lock_tx_fee}
              quote={quote}
            />
          </Box>
        }
        icon={<BitcoinIcon />}
      />
    </Box>
  );
}
