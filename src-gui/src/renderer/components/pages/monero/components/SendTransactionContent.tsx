import {
  Button,
  Box,
  DialogActions,
  DialogContent,
  DialogTitle,
  Typography,
} from "@mui/material";
import { useState } from "react";
import { xmrToPiconeros } from "../../../../../utils/conversionUtils";
import SendAmountInput from "./SendAmountInput";
import MoneroAddressTextField from "renderer/components/inputs/MoneroAddressTextField";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { sendMoneroTransaction } from "renderer/rpc";
import { useAppSelector } from "store/hooks";
import { SendMoneroResponse } from "models/tauriModel";

interface SendTransactionContentProps {
  balance: {
    unlocked_balance: string;
  };
  onClose: () => void;
  onSuccess: (response: SendMoneroResponse) => void;
}

export default function SendTransactionContent({
  balance,
  onSuccess,
  onClose,
}: SendTransactionContentProps) {
  const [sendAddress, setSendAddress] = useState("");
  const [sendAmount, setSendAmount] = useState("");
  const [previousAmount, setPreviousAmount] = useState("");
  const [enableSend, setEnableSend] = useState(false);
  const [currency, setCurrency] = useState("XMR");
  const [isMaxSelected, setIsMaxSelected] = useState(false);
  const [isSending, setIsSending] = useState(false);

  const showFiatRate = useAppSelector(
    (state) => state.settings.fetchFiatPrices,
  );
  const fiatCurrency = useAppSelector((state) => state.settings.fiatCurrency);
  const xmrPrice = useAppSelector((state) => state.rates.xmrPrice);

  const handleCurrencyChange = (newCurrency: string) => {
    if (!showFiatRate || !xmrPrice || isMaxSelected || isSending) {
      return;
    }

    if (sendAmount === "" || parseFloat(sendAmount) === 0) {
      setSendAmount(newCurrency === "XMR" ? "0.000" : "0.00");
    } else {
      setSendAmount(
        newCurrency === "XMR"
          ? (parseFloat(sendAmount) / xmrPrice).toFixed(3)
          : (parseFloat(sendAmount) * xmrPrice).toFixed(2),
      );
    }
    setCurrency(newCurrency);
  };

  const handleMaxToggled = () => {
    if (isSending) return;

    if (isMaxSelected) {
      // Disable MAX mode - restore previous amount
      setIsMaxSelected(false);
      setSendAmount(previousAmount);
    } else {
      // Enable MAX mode - save current amount first
      setPreviousAmount(sendAmount);
      setIsMaxSelected(true);
      setSendAmount("<MAX>");
    }
  };

  const handleAmountChange = (newAmount: string) => {
    if (isSending) return;

    if (newAmount !== "<MAX>") {
      setIsMaxSelected(false);
    }
    setSendAmount(newAmount);
  };

  const handleAddressChange = (newAddress: string) => {
    if (isSending) return;
    setSendAddress(newAddress);
  };

  const moneroAmount =
    currency === "XMR"
      ? parseFloat(sendAmount)
      : parseFloat(sendAmount) / xmrPrice;

  const handleSend = async () => {
    if (!sendAddress) {
      throw new Error("Address is required");
    }

    if (isMaxSelected) {
      return sendMoneroTransaction({
        address: sendAddress,
        amount: { type: "Sweep" },
      });
    } else {
      if (!sendAmount || sendAmount === "<MAX>") {
        throw new Error("Amount is required");
      }

      return sendMoneroTransaction({
        address: sendAddress,
        amount: {
          type: "Specific",
          // Floor the amount to avoid rounding decimal amounts
          // The amount is in piconeros, so it NEEDS to be a whole number
          amount: Math.floor(xmrToPiconeros(moneroAmount)),
        },
      });
    }
  };

  const handleSendSuccess = (response: SendMoneroResponse) => {
    // Clear form after successful send
    handleClear();
    onSuccess(response);
  };

  const handleClear = () => {
    setSendAddress("");
    setSendAmount("");
    setPreviousAmount("");
    setIsMaxSelected(false);
  };

  const isSendDisabled =
    !enableSend || (!isMaxSelected && (!sendAmount || sendAmount === "<MAX>"));

  return (
    <>
      <DialogTitle>Send</DialogTitle>
      <DialogContent>
        <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <SendAmountInput
            balance={balance}
            amount={sendAmount}
            onAmountChange={handleAmountChange}
            onMaxToggled={handleMaxToggled}
            currency={currency}
            fiatCurrency={fiatCurrency}
            xmrPrice={xmrPrice}
            showFiatRate={showFiatRate}
            onCurrencyChange={handleCurrencyChange}
            disabled={isSending}
          />
          <MoneroAddressTextField
            address={sendAddress}
            onAddressChange={handleAddressChange}
            onAddressValidityChange={setEnableSend}
            label="Send to"
            fullWidth
            disabled={isSending}
          />
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <PromiseInvokeButton
          onInvoke={handleSend}
          disabled={isSendDisabled}
          onSuccess={handleSendSuccess}
          onPendingChange={setIsSending}
        >
          Send
        </PromiseInvokeButton>
      </DialogActions>
    </>
  );
}
