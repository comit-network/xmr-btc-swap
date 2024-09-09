import {
  Box,
  Collapse,
  IconButton,
  makeStyles,
  TableCell,
  TableRow,
} from "@material-ui/core";
import ArrowForwardIcon from "@material-ui/icons/ArrowForward";
import KeyboardArrowDownIcon from "@material-ui/icons/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@material-ui/icons/KeyboardArrowUp";
import { GetSwapInfoResponse } from "models/tauriModel";
import { useState } from "react";
import TruncatedText from "renderer/components/other/TruncatedText";
import { PiconeroAmount, SatsAmount } from "../../../other/Units";
import HistoryRowActions from "./HistoryRowActions";
import HistoryRowExpanded from "./HistoryRowExpanded";

const useStyles = makeStyles((theme) => ({
  amountTransferContainer: {
    display: "flex",
    alignItems: "center",
    gap: theme.spacing(1),
  },
}));

function AmountTransfer({
  btcAmount,
  xmrAmount,
}: {
  xmrAmount: number;
  btcAmount: number;
}) {
  const classes = useStyles();

  return (
    <Box className={classes.amountTransferContainer}>
      <SatsAmount amount={btcAmount} />
      <ArrowForwardIcon />
      <PiconeroAmount amount={xmrAmount} />
    </Box>
  );
}

export default function HistoryRow(swap: GetSwapInfoResponse) {
  const [expanded, setExpanded] = useState(false);

  return (
    <>
      <TableRow>
        <TableCell>
          <IconButton size="small" onClick={() => setExpanded(!expanded)}>
            {expanded ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </IconButton>
        </TableCell>
        <TableCell>
          <TruncatedText>{swap.swap_id}</TruncatedText>
        </TableCell>
        <TableCell>
          <AmountTransfer
            xmrAmount={swap.xmr_amount}
            btcAmount={swap.btc_amount}
          />
        </TableCell>
        <TableCell>{swap.state_name.toString()}</TableCell>
        <TableCell>
          <HistoryRowActions {...swap} />
        </TableCell>
      </TableRow>

      <TableRow>
        <TableCell style={{ padding: 0 }} colSpan={6}>
          <Collapse in={expanded} timeout="auto">
            {expanded && <HistoryRowExpanded swap={swap} />}
          </Collapse>
        </TableCell>
      </TableRow>
    </>
  );
}
