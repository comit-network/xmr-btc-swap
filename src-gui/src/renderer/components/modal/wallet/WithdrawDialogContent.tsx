import { Box, DialogContent } from "@mui/material";
import { ReactNode } from "react";
import WithdrawStepper from "./WithdrawStepper";

export default function WithdrawDialogContent({
  children,
  isPending,
  withdrawTxId,
}: {
  children: ReactNode;
  isPending: boolean;
  withdrawTxId: string | null;
}) {
  return (
    <DialogContent
      dividers
      sx={{
        minHeight: "15rem",
        display: "flex",
        flexDirection: "column",
        justifyContent: "space-between",
      }}
    >
      <Box>{children}</Box>
      <WithdrawStepper isPending={isPending} withdrawTxId={withdrawTxId} />
    </DialogContent>
  );
}
