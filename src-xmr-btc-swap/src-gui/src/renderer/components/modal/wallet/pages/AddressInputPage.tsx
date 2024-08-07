import { useState } from 'react';
import { Button, DialogActions, DialogContentText } from '@material-ui/core';
import BitcoinAddressTextField from '../../../inputs/BitcoinAddressTextField';
import WithdrawDialogContent from '../WithdrawDialogContent';
import IpcInvokeButton from '../../../IpcInvokeButton';

export default function AddressInputPage({
  onCancel,
}: {
  onCancel: () => void;
}) {
  const [withdrawAddressValid, setWithdrawAddressValid] = useState(false);
  const [withdrawAddress, setWithdrawAddress] = useState('');

  return (
    <>
      <WithdrawDialogContent>
        <DialogContentText>
          To withdraw the BTC of the internal wallet, please enter an address.
          All funds will be sent to that address.
        </DialogContentText>

        <BitcoinAddressTextField
          address={withdrawAddress}
          onAddressChange={setWithdrawAddress}
          onAddressValidityChange={setWithdrawAddressValid}
          helperText="All Bitcoin of the internal wallet will be transferred to this address"
          fullWidth
        />
      </WithdrawDialogContent>

      <DialogActions>
        <Button onClick={onCancel} variant="text">
          Cancel
        </Button>
        <IpcInvokeButton
          disabled={!withdrawAddressValid}
          ipcChannel="spawn-withdraw-btc"
          ipcArgs={[withdrawAddress]}
          color="primary"
          variant="contained"
          requiresRpc
        >
          Withdraw
        </IpcInvokeButton>
      </DialogActions>
    </>
  );
}
