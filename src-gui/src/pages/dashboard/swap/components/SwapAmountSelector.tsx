import { Box, TextField, Tooltip, Typography } from "@mui/material";
import ArrowForwardIcon from "@mui/icons-material/ArrowForward";
import { useEffect, useState } from "react";
import { useAppDispatch, useAppSelector } from "store/hooks";
import FiatPriceLabel from "./FiatPriceLabel";
import { Currency } from "./FiatPriceLabel";
import { setBtcAmount } from "store/features/startSwapSlice";

export default function SwapAmountSelector({
  fullWidth,
}: {
  fullWidth?: boolean;
}) {
  const xmrBtcRate = useAppSelector((state) => state.rates.xmrBtcRate);
  const btcAmount = useAppSelector((state) => state.startSwap.btcAmount);
  const dispatch = useAppDispatch();
  const [amounts, setAmounts] = useState<{ btc: number; xmr: number }>({ 
    btc: btcAmount, 
    xmr: 0
  });

  // Update BTC amount when XMR changes
  useEffect(() => {
    if (xmrBtcRate && amounts.xmr !== undefined && amounts.xmr !== null) {
      const newBtc = Number((amounts.xmr * xmrBtcRate).toFixed(8));
      dispatch(setBtcAmount(newBtc));
      setAmounts(prev => ({
        ...prev,
        btc: newBtc
      }));
    }
  }, [amounts.xmr, xmrBtcRate]);

  // Update XMR amount when BTC changes
  useEffect(() => {
    if (xmrBtcRate && amounts.btc !== undefined && amounts.btc !== null) {
      dispatch(setBtcAmount(amounts.btc));
      const newXmr = Number((amounts.btc / xmrBtcRate).toFixed(12));
      setAmounts(prev => ({
        ...prev,
        xmr: newXmr
      }));
    }
  }, [amounts.btc, xmrBtcRate]);

  return (
    <Box
      sx={{
        display: "grid",
        gridTemplateColumns: "1fr auto 1fr",
        alignItems: "center",
        gap: 1,
        width: fullWidth ? "100%" : "auto",
      }}
    >
      <TextField
        label="BTC"
        fullWidth={fullWidth}
        value={amounts.btc.toFixed(5)}
        onChange={(e) => {
          const value = Number(e.target.value);
          if (!isNaN(value)) {
            setAmounts(prev => ({ ...prev, btc: value }));
          }
        }}
        type="number"
        slotProps={{
          htmlInput: {
            inputMode: 'decimal',
            step: '0.00001',
            min: '0'
          }
        }}
        sx={{
          gridColumn: "1 / 2",
          gridRow: "2",
        }}
      />
      <FiatPriceLabel
        amount={amounts.btc}
        originalCurrency={Currency.BTC}
        gridColumn="1 / 2"
        gridRow="3"
      />

      <ArrowForwardIcon
        sx={{
          justifySelf: "center",
          gridColumn: "2 / 3",
          gridRow: "2",
        }}
      />

      <Tooltip
        title="The actual Monero amount might vary slightly"
        enterDelay={1500}
        enterNextDelay={500}
        leaveDelay={500}
        placement="top"
      >
        <TextField
          label="XMR"
          fullWidth={fullWidth}
          value={amounts.xmr.toFixed(5)}
          onChange={(e) => {
            const value = Number(e.target.value);
            if (!isNaN(value)) {
              setAmounts(prev => ({ ...prev, xmr: value }));
            }
          }}
          type="number"
          slotProps={{
            htmlInput: {
              inputMode: 'decimal',
              step: '0.00001',
              min: '0'
            }
          }}
          sx={{
            gridColumn: "3 / 4",
            gridRow: "2",
          }}
        />
      </Tooltip>

      <FiatPriceLabel
        amount={amounts.xmr}
        originalCurrency={Currency.XMR}
        gridColumn="3 / 4"
        gridRow="3"
      />
    </Box>
  );
}
