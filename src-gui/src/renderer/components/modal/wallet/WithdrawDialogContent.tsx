import { Box, DialogContent, makeStyles } from "@material-ui/core";
import { ReactNode } from "react";
import WithdrawStepper from "./WithdrawStepper";

const useStyles = makeStyles({
  outer: {
    minHeight: "15rem",
    display: "flex",
    flexDirection: "column",
    justifyContent: "space-between",
  },
});

export default function WithdrawDialogContent({
  children,
  isPending,
  withdrawTxId,
}: {
  children: ReactNode;
  isPending: boolean;
  withdrawTxId: string | null;
}) {
  const classes = useStyles();

  return (
    <DialogContent dividers className={classes.outer}>
      <Box>{children}</Box>
      <WithdrawStepper isPending={isPending} withdrawTxId={withdrawTxId} />
    </DialogContent>
  );
}
