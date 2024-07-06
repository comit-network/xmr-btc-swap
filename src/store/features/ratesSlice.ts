import { createSlice, PayloadAction } from '@reduxjs/toolkit';

export interface RatesState {
  btcPrice: number | null;
  xmrPrice: number | null;
}

const initialState: RatesState = {
  btcPrice: null,
  xmrPrice: null,
};

const ratesSlice = createSlice({
  name: 'rates',
  initialState,
  reducers: {
    setBtcPrice: (state, action: PayloadAction<number>) => {
      state.btcPrice = action.payload;
    },
    setXmrPrice: (state, action: PayloadAction<number>) => {
      state.xmrPrice = action.payload;
    },
  },
});

export const { setBtcPrice, setXmrPrice } = ratesSlice.actions;

export default ratesSlice.reducer;
