import { DialogTitle, Typography } from "@mui/material";
import { ReactNode } from "react";

type DialogTitleProps = {
  title: ReactNode;
};

export default function DialogHeader({ title }: DialogTitleProps) {
  return (
    <DialogTitle
      sx={{
        display: "flex",
        justifyContent: "space-between",
      }}
    >
      <Typography variant="h6">{title}</Typography>
    </DialogTitle>
  );
}
