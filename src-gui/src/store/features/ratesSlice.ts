import { createSlice, PayloadAction } from "@reduxjs/toolkit";

export interface RatesState {
  // USD price of 1 BTC
  btcPrice: number | null;
  // USD price of 1 XMR
  xmrPrice: number | null;
  // XMR/BTC exchange rate
  xmrBtcRate: number | null;
}

const initialState: RatesState = {
  btcPrice: null,
  xmrPrice: null,
  xmrBtcRate: null,
};

const ratesSlice = createSlice({
  name: "rates",
  initialState,
  reducers: {
    setBtcPrice: (state, action: PayloadAction<number>) => {
      state.btcPrice = action.payload;
    },
    setXmrPrice: (state, action: PayloadAction<number>) => {
      state.xmrPrice = action.payload;
    },
    setXmrBtcRate: (state, action: PayloadAction<number>) => {
      state.xmrBtcRate = action.payload;
    },
  },
});

export const { setBtcPrice, setXmrPrice, setXmrBtcRate } = ratesSlice.actions;

export default ratesSlice.reducer;
