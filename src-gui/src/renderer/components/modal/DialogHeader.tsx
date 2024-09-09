import { DialogTitle, makeStyles, Typography } from "@material-ui/core";
import { ReactNode } from "react";

const useStyles = makeStyles({
  root: {
    display: "flex",
    justifyContent: "space-between",
  },
});

type DialogTitleProps = {
  title: ReactNode;
};

export default function DialogHeader({ title }: DialogTitleProps) {
  const classes = useStyles();

  return (
    <DialogTitle disableTypography className={classes.root}>
      <Typography variant="h6">{title}</Typography>
    </DialogTitle>
  );
}
