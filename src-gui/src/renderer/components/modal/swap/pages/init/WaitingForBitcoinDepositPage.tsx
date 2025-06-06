import { Box, Typography } from "@mui/material";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import BitcoinIcon from "../../../../icons/BitcoinIcon";
import { MoneroSatsExchangeRate, SatsAmount } from "../../../../other/Units";
import DepositAddressInfoBox from "../../DepositAddressInfoBox";
import DepositAmountHelper from "./DepositAmountHelper";
import { Alert } from "@mui/material";

export default function WaitingForBtcDepositPage({
  deposit_address,
  min_deposit_until_swap_will_start,
  max_deposit_until_maximum_amount_is_reached,
  min_bitcoin_lock_tx_fee,
  max_giveable,
  quote,
}: TauriSwapProgressEventContent<"WaitingForBtcDeposit">) {
  return (
    <Box>
      <DepositAddressInfoBox
        title="Bitcoin Deposit Address"
        address={deposit_address}
        additionalContent={
          <Box
            sx={{
              paddingTop: 1,
              gap: 0.5,
              display: "flex",
              flexDirection: "column",
            }}
          >
            <Typography variant="subtitle2">
              <ul>
                {max_giveable > 0 ? (
                  <li>
                    You have already deposited enough funds to swap{" "}
                    <SatsAmount amount={max_giveable} />. However, that is below
                    the minimum amount required to start the swap.
                  </li>
                ) : null}
                <li>
                  Send any amount between{" "}
                  <SatsAmount amount={min_deposit_until_swap_will_start} /> and{" "}
                  <SatsAmount
                    amount={max_deposit_until_maximum_amount_is_reached}
                  />{" "}
                  to the address above
                  {max_giveable > 0 && (
                    <> (on top of the already deposited funds)</>
                  )}
                </li>
                <li>
                  Bitcoin sent to this this address will be converted into
                  Monero at an exchange rate of{" ≈ "}
                  <MoneroSatsExchangeRate
                    rate={quote.price}
                    displayMarkup={true}
                  />
                </li>
                <li>
                  The Network fee of{" ≈  "}
                  <SatsAmount amount={min_bitcoin_lock_tx_fee} /> will
                  automatically be deducted from the deposited coins
                </li>
                <li>
                  After the deposit is detected, you'll get to confirm the exact
                  details before your funds are locked
                </li>
                <li>
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
                </li>
              </ul>
            </Typography>

            <Alert severity="info">
              Please do not use replace-by-fee on your deposit transaction.
              You'll need to start a new swap if you do. The funds will be
              available for future swaps.
            </Alert>
          </Box>
        }
        icon={<BitcoinIcon />}
      />
    </Box>
  );
}
