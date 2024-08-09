import { useState } from "react";
import { Button, DialogActions, DialogContentText } from "@material-ui/core";
import BitcoinAddressTextField from "../../../inputs/BitcoinAddressTextField";
import WithdrawDialogContent from "../WithdrawDialogContent";
import IpcInvokeButton from "../../../IpcInvokeButton";

export default function AddressInputPage({
  withdrawAddress,
  setWithdrawAddress,
  setWithdrawAddressValid,
}: {
  withdrawAddress: string;
  setWithdrawAddress: (address: string) => void;
  setWithdrawAddressValid: (valid: boolean) => void;
}) {
  return (
    <>
      <DialogContentText>
        To withdraw the Bitcoin inside the internal wallet, please enter an
        address. All funds will be sent to that address.
      </DialogContentText>

      <BitcoinAddressTextField
        address={withdrawAddress}
        onAddressChange={setWithdrawAddress}
        onAddressValidityChange={setWithdrawAddressValid}
        helperText="All Bitcoin of the internal wallet will be transferred to this address"
        fullWidth
      />
    </>
  );
}
