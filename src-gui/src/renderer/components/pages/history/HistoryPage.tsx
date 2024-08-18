import { Typography } from "@material-ui/core";
import { useIsSwapRunning } from "store/hooks";
import SwapTxLockAlertsBox from "../../alert/SwapTxLockAlertsBox";
import SwapDialog from "../../modal/swap/SwapDialog";
import HistoryTable from "./table/HistoryTable";

export default function HistoryPage() {
  const showDialog = useIsSwapRunning();

  return (
    <>
      <Typography variant="h3">History</Typography>
      <SwapTxLockAlertsBox />
      <HistoryTable />
      <SwapDialog open={showDialog} onClose={() => {}} />
    </>
  );
}
