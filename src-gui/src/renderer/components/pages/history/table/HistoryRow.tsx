import { Box, Collapse, IconButton, TableCell, TableRow } from "@mui/material";
import ArrowForwardIcon from "@mui/icons-material/ArrowForward";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import { GetSwapInfoResponse } from "models/tauriModel";
import { useState } from "react";
import TruncatedText from "renderer/components/other/TruncatedText";
import { PiconeroAmount, SatsAmount } from "../../../other/Units";
import HistoryRowActions from "./HistoryRowActions";
import HistoryRowExpanded from "./HistoryRowExpanded";
import {
  bobStateNameToHumanReadable,
  GetSwapInfoResponseExt,
} from "models/tauriModelExt";

function AmountTransfer({
  btcAmount,
  xmrAmount,
}: {
  xmrAmount: number;
  btcAmount: number;
}) {
  return (
    <Box
      sx={{
        display: "flex",
        alignItems: "center",
        gap: 1,
      }}
    >
      <SatsAmount amount={btcAmount} />
      <ArrowForwardIcon />
      <PiconeroAmount amount={xmrAmount} />
    </Box>
  );
}

export default function HistoryRow(swap: GetSwapInfoResponseExt) {
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
        <TableCell>{bobStateNameToHumanReadable(swap.state_name)}</TableCell>
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
