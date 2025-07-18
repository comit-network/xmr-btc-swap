import {
  Box,
  Paper,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Typography,
  Skeleton,
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
          {swapSortedByDate.length > 0 && (
            <TableHead>
              <TableRow>
                <TableCell />
                <TableCell>ID</TableCell>
                <TableCell>Amount</TableCell>
                <TableCell>State</TableCell>
                <TableCell />
              </TableRow>
            </TableHead>
          )}
          <TableBody>
            {swapSortedByDate.length === 0 ? (
              <>
                <TableRow>
                  <TableCell colSpan={5} sx={{ textAlign: "center", py: 4 }}>
                    <Typography
                      variant="h6"
                      color="text.secondary"
                      gutterBottom
                    >
                      Nothing to see here
                    </Typography>
                    <Typography variant="body2" color="text.secondary">
                      You haven't made any swaps yet
                    </Typography>
                  </TableCell>
                </TableRow>
                {/* Skeleton rows for visual loading effect */}
                {Array.from({ length: 3 }).map((_, index) => (
                  <TableRow key={index}>
                    <TableCell>
                      <Skeleton
                        animation={false}
                        variant="circular"
                        width={24}
                        height={24}
                      />
                    </TableCell>
                    <TableCell>
                      <Skeleton animation={false} variant="text" width="80%" />
                    </TableCell>
                    <TableCell>
                      <Skeleton animation={false} variant="text" width="60%" />
                    </TableCell>
                    <TableCell>
                      <Skeleton
                        animation={false}
                        variant="rectangular"
                        width={80}
                        height={24}
                      />
                    </TableCell>
                    <TableCell>
                      <Skeleton
                        animation={false}
                        variant="circular"
                        width={24}
                        height={24}
                      />
                    </TableCell>
                  </TableRow>
                ))}
              </>
            ) : (
              swapSortedByDate.map((swap) => (
                <HistoryRow {...swap} key={swap.swap_id} />
              ))
            )}
          </TableBody>
        </Table>
      </TableContainer>
    </Box>
  );
}
