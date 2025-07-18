import { Typography, Box, Paper } from "@mui/material";
import { TransactionInfo } from "models/tauriModel";
import _ from "lodash";
import dayjs from "dayjs";
import TransactionItem from "./TransactionItem";

interface TransactionHistoryProps {
  history?: {
    transactions: TransactionInfo[];
  };
}

interface TransactionGroup {
  date: string;
  displayDate: string;
  transactions: TransactionInfo[];
}

// Component for displaying transaction history
export default function TransactionHistory({
  history,
}: TransactionHistoryProps) {
  if (!history || !history.transactions || history.transactions.length === 0) {
    return <Typography variant="h5">Transactions</Typography>;
  }

  const transactions = history.transactions;

  // Group transactions by date using dayjs and lodash
  const transactionGroups: TransactionGroup[] = _(transactions)
    .groupBy((tx) => dayjs(tx.timestamp * 1000).format("YYYY-MM-DD")) // Convert Unix timestamp to date string
    .map((txs, dateKey) => ({
      date: dateKey,
      displayDate: dayjs(dateKey).format("MMMM D, YYYY"), // Human-readable format
      transactions: _.orderBy(txs, ["timestamp"], ["desc"]), // Sort transactions within group by newest first
    }))
    .orderBy(["date"], ["desc"]) // Sort groups by newest date first
    .value();

  return (
    <Box>
      <Typography variant="h5" sx={{ mb: 2 }}>
        Transactions
      </Typography>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {transactionGroups.map((group) => (
          <Box key={group.date}>
            <Typography variant="body1" color="text.secondary" sx={{ mb: 1 }}>
              {group.displayDate}
            </Typography>
            <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
              {group.transactions.map((tx) => (
                <TransactionItem key={tx.tx_hash} transaction={tx} />
              ))}
            </Box>
          </Box>
        ))}
      </Box>
    </Box>
  );
}
