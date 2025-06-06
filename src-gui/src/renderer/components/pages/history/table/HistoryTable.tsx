import {
  Box,
  Paper,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
} from "@mui/material";
import { useSwapInfosSortedByDate } from "../../../../../store/hooks";
import HistoryRow from "./HistoryRow";

export default function HistoryTable() {
  const swapSortedByDate = useSwapInfosSortedByDate();

  return (
    <Box
      sx={{
        paddingTop: 1,
        paddingBottom: 1,
      }}
    >
      <TableContainer component={Paper}>
        <Table>
          <TableHead>
            <TableRow>
              <TableCell />
              <TableCell>ID</TableCell>
              <TableCell>Amount</TableCell>
              <TableCell>State</TableCell>
              <TableCell />
            </TableRow>
          </TableHead>
          <TableBody>
            {swapSortedByDate.map((swap) => (
              <HistoryRow {...swap} key={swap.swap_id} />
            ))}
          </TableBody>
        </Table>
      </TableContainer>
    </Box>
  );
}
