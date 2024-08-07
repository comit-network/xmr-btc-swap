import { ReactNode } from 'react';
import { Box, DialogContent, makeStyles } from '@material-ui/core';
import WithdrawStepper from './WithdrawStepper';

const useStyles = makeStyles({
  outer: {
    minHeight: '15rem',
    display: 'flex',
    flexDirection: 'column',
    justifyContent: 'space-between',
  },
});

export default function WithdrawDialogContent({
  children,
}: {
  children: ReactNode;
}) {
  const classes = useStyles();

  return (
    <DialogContent dividers className={classes.outer}>
      <Box>{children}</Box>
      <WithdrawStepper />
    </DialogContent>
  );
}
