import {
  Box,
  makeStyles,
  Paper,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
} from "@material-ui/core";
import { useSwapInfosSortedByDate } from "../../../../../store/hooks";
import HistoryRow from "./HistoryRow";

const useStyles = makeStyles((theme) => ({
  outer: {
    paddingTop: theme.spacing(1),
    paddingBottom: theme.spacing(1),
  },
}));

export default function HistoryTable() {
  const classes = useStyles();
  const swapSortedByDate = useSwapInfosSortedByDate();

  return (
    <Box className={classes.outer}>
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
