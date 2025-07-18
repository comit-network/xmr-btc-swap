import { Typography } from "@mui/material";
import SwapTxLockAlertsBox from "../../alert/SwapTxLockAlertsBox";
import HistoryTable from "./table/HistoryTable";

export default function HistoryPage() {
  return (
    <>
      <SwapTxLockAlertsBox />
      <HistoryTable />
    </>
  );
}
