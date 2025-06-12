import { Autocomplete, Box, TextField, Typography } from "@mui/material";
import { useState, useEffect } from "react";
import { getMoneroAddresses } from "renderer/rpc";
import { isTestnet } from "store/config";
import { isXmrAddressValid } from "utils/conversionUtils";
import { useAppDispatch, useAppSelector } from "store/hooks";
import { setRedeemAddress } from "store/features/startSwapSlice";

export default function ReceiveAddressSelector() {
  const [addresses, setAddresses] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const redeemAddress = useAppSelector((state) => state.startSwap.redeemAddress);
  const dispatch = useAppDispatch();

  useEffect(() => {
    if (redeemAddress) {
      const isValid = isXmrAddressValid(redeemAddress, isTestnet());
      if (!isValid) {
        setError("Invalid Monero address");
      } else {
        setError(null);
      }
    }
  }, [redeemAddress])

  useEffect(() => {
    const fetchAddresses = async () => {
      const response = await getMoneroAddresses();
      setAddresses(response.addresses);
    };
    fetchAddresses();
  }, []);

  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "row",
        alignItems: "center",
        gap: 2,
        width: "100%",
        marginTop: 1,
      }}
    >
      <Typography variant="body1">Receive Address</Typography>
      <Autocomplete
        sx={{
          flexGrow: 1,
        }}
        freeSolo
        options={addresses}
        value={redeemAddress}
        onChange={(_, value) => dispatch(setRedeemAddress(value))}
        onInputChange={(_, value) => dispatch(setRedeemAddress(value))}
        renderInput={(params) => (
          <TextField {...params} label="Receive Address" fullWidth error={!!error} helperText={error} />
        )}
      />
    </Box>
  );
}
