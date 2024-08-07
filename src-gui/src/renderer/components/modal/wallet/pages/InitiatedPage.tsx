import { Button, DialogActions } from '@material-ui/core';
import CircularProgressWithSubtitle from '../../swap/CircularProgressWithSubtitle';
import WithdrawDialogContent from '../WithdrawDialogContent';

export default function InitiatedPage({ onCancel }: { onCancel: () => void }) {
  return (
    <>
      <WithdrawDialogContent>
        <CircularProgressWithSubtitle description="Withdrawing Bitcoin" />
      </WithdrawDialogContent>
      <DialogActions>
        <Button onClick={onCancel} variant="text">
          Cancel
        </Button>
        <Button disabled color="primary" variant="contained">
          Done
        </Button>
      </DialogActions>
    </>
  );
}
