import {
  Box,
  Link,
  makeStyles,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableRow,
} from "@material-ui/core";
import { OpenInNew } from "@material-ui/icons";
import { GetSwapInfoResponse } from "models/tauriModel";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import MonospaceTextBox from "renderer/components/other/MonospaceTextBox";
import {
  MoneroBitcoinExchangeRate,
  MoneroBitcoinExchangeRateFromAmounts,
  PiconeroAmount,
  SatsAmount,
} from "renderer/components/other/Units";
import { isTestnet } from "store/config";
import { getBitcoinTxExplorerUrl } from "utils/conversionUtils";
import SwapLogFileOpenButton from "./SwapLogFileOpenButton";

const useStyles = makeStyles((theme) => ({
  outer: {
    display: "grid",
    padding: theme.spacing(1),
    gap: theme.spacing(1),
  },
  outerAddressBox: {
    display: "flex",
    flexDirection: "column",
    gap: theme.spacing(1),
  },
  actionsOuter: {
    display: "flex",
    flexDirection: "row",
    gap: theme.spacing(1),
  },
}));

export default function HistoryRowExpanded({
  swap,
}: {
  swap: GetSwapInfoResponse;
}) {
  const classes = useStyles();

  return (
    <Box className={classes.outer}>
      <TableContainer>
        <Table>
          <TableBody>
            <TableRow>
              <TableCell>Started on</TableCell>
              <TableCell>{swap.start_date}</TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Swap ID</TableCell>
              <TableCell>{swap.swap_id}</TableCell>
            </TableRow>
            <TableRow>
              <TableCell>State Name</TableCell>
              <TableCell>{swap.state_name}</TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Monero Amount</TableCell>
              <TableCell>
                <PiconeroAmount amount={swap.xmr_amount} />
              </TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Bitcoin Amount</TableCell>
              <TableCell>
                <SatsAmount amount={swap.btc_amount} />
              </TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Exchange Rate</TableCell>
              <TableCell>
                <MoneroBitcoinExchangeRateFromAmounts
                  satsAmount={swap.btc_amount}
                  piconeroAmount={swap.xmr_amount}
                />
              </TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Bitcoin Network Fees</TableCell>
              <TableCell>
                <SatsAmount amount={swap.tx_lock_fee} />
              </TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Maker Address</TableCell>
              <TableCell>
                <Box className={classes.outerAddressBox}>
                  {swap.seller.addresses.map((addr) => (
                    <ActionableMonospaceTextBox
                      key={addr}
                      content={addr}
                      displayCopyIcon={true}
                      enableQrCode={false}
                    />
                  ))}
                </Box>
              </TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Bitcoin lock transaction</TableCell>
              <TableCell>
                <Link
                  href={getBitcoinTxExplorerUrl(swap.tx_lock_id, isTestnet())}
                  target="_blank"
                >
                  <MonospaceTextBox>
                    {swap.tx_lock_id}
                  </MonospaceTextBox>
                </Link>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </TableContainer>
      <Box className={classes.actionsOuter}>
        <SwapLogFileOpenButton
          swapId={swap.swap_id}
          variant="outlined"
          size="small"
        />
      </Box>
    </Box>
  );
}
