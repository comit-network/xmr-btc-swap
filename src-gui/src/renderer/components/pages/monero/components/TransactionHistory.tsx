import {
  Typography,
  Card,
  CardContent,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Paper,
  Chip,
  IconButton,
  Tooltip,
  Stack,
} from "@mui/material";
import { OpenInNew as OpenInNewIcon } from "@mui/icons-material";
import { open } from "@tauri-apps/plugin-shell";
import { PiconeroAmount } from "../../../other/Units";
import { getMoneroTxExplorerUrl } from "../../../../../utils/conversionUtils";
import { isTestnet } from "store/config";
import { TransactionInfo } from "models/tauriModel";

interface TransactionHistoryProps {
  history?: {
    transactions: TransactionInfo[];
  };
}

// Component for displaying transaction history
export default function TransactionHistory({
  history,
}: TransactionHistoryProps) {
  if (!history || !history.transactions || history.transactions.length === 0) {
    return <Typography variant="h5">Transaction History</Typography>;
  }

  return (
    <>
      <Typography variant="h5">Transaction History</Typography>

      <TableContainer component={Paper} variant="outlined">
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell>Amount</TableCell>
              <TableCell>Fee</TableCell>
              <TableCell align="right">Confirmations</TableCell>
              <TableCell align="center">Explorer</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {[...history.transactions]
              .sort((a, b) => a.confirmations - b.confirmations)
              .map((tx, index) => (
                <TableRow key={index}>
                  <TableCell>
                    <Stack direction="row" spacing={1} alignItems="center">
                      <PiconeroAmount amount={tx.amount} />
                      <Chip
                        label={tx.direction === "In" ? "Received" : "Sent"}
                        color={tx.direction === "In" ? "success" : "default"}
                        size="small"
                      />
                    </Stack>
                  </TableCell>
                  <TableCell>
                    <PiconeroAmount amount={tx.fee} />
                  </TableCell>
                  <TableCell align="right">
                    <Chip
                      label={tx.confirmations}
                      color={tx.confirmations >= 10 ? "success" : "warning"}
                      size="small"
                    />
                  </TableCell>
                  <TableCell align="center">
                    {tx.tx_hash && (
                      <Tooltip title="View on block explorer">
                        <IconButton
                          size="small"
                          onClick={() => {
                            const url = getMoneroTxExplorerUrl(
                              tx.tx_hash,
                              isTestnet(),
                            );
                            open(url);
                          }}
                        >
                          <OpenInNewIcon fontSize="small" />
                        </IconButton>
                      </Tooltip>
                    )}
                  </TableCell>
                </TableRow>
              ))}
          </TableBody>
        </Table>
      </TableContainer>
    </>
  );
}
