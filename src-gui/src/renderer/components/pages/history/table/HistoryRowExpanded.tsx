import {
  Box,
  Link,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableRow,
  Typography,
} from "@mui/material";
import { GetSwapInfoResponse } from "models/tauriModel";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import MonospaceTextBox from "renderer/components/other/MonospaceTextBox";
import {
  MoneroBitcoinExchangeRateFromAmounts,
  PiconeroAmount,
  SatsAmount,
} from "renderer/components/other/Units";
import { isTestnet } from "store/config";
import { getBitcoinTxExplorerUrl } from "utils/conversionUtils";
import SwapLogFileOpenButton from "./SwapLogFileOpenButton";
import ExportLogsButton from "./ExportLogsButton";

export default function HistoryRowExpanded({
  swap,
}: {
  swap: GetSwapInfoResponse;
}) {
  return (
    <Box
      sx={{
        display: "grid",
        padding: 1,
        gap: 1,
      }}
    >
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
                <Box
                  sx={{
                    display: "flex",
                    flexDirection: "column",
                    gap: 1,
                  }}
                >
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
                  <MonospaceTextBox>{swap.tx_lock_id}</MonospaceTextBox>
                </Link>
              </TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Monero receive pool</TableCell>
              <TableCell>
                <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
                  {swap.monero_receive_pool.map((pool, index) => (
                    <Box
                      key={index}
                      sx={{
                        display: "flex",
                        flexDirection: "column",
                        gap: 0.5,
                        padding: 1,
                        border: 1,
                        borderColor: "divider",
                        borderRadius: 1,
                        backgroundColor: (theme) => theme.palette.action.hover,
                      }}
                    >
                      <Typography
                        variant="body2"
                        sx={(theme) => ({
                          fontWeight: 600,
                          color: theme.palette.text.primary,
                        })}
                      >
                        {pool.label} ({pool.percentage * 100}%)
                      </Typography>
                      <Typography
                        variant="caption"
                        sx={{
                          fontFamily: "monospace",
                          color: (theme) => theme.palette.text.secondary,
                          wordBreak: "break-all",
                        }}
                      >
                        {pool.address}
                      </Typography>
                    </Box>
                  ))}
                </Box>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </TableContainer>
      <Box
        sx={{
          display: "flex",
          flexDirection: "row",
          gap: 1,
        }}
      >
        <SwapLogFileOpenButton
          swapId={swap.swap_id}
          variant="outlined"
          size="small"
        />
        <ExportLogsButton
          swap_id={swap.swap_id}
          variant="outlined"
          size="small"
        />
      </Box>
    </Box>
  );
}
