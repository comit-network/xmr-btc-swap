import { createSlice, PayloadAction } from "@reduxjs/toolkit";

export enum StartSwapStep {
  DepositBitcoin = "depositBitcoin",
  SelectMaker = "selectMaker",
  ReviewOffer = "reviewOffer",
}

export interface StartSwapSlice {
  step: StartSwapStep;
  btcAmount: number;
  redeemAddress: string;
  offer: null;
  maker: null;
}

const initialState: StartSwapSlice = {
  step: StartSwapStep.DepositBitcoin,
  btcAmount: 0,
  redeemAddress: "",
  offer: null,
  maker: null,
};

export const startSwapSlice = createSlice({
  name: "startSwap",
  initialState,
  reducers: {
    setBtcAmount(state, action: PayloadAction<number>) {
      state.btcAmount = action.payload;
    },
    setRedeemAddress(state, action: PayloadAction<string>) {
      state.redeemAddress = action.payload;
    },
    setOffer(state, action: PayloadAction<any>) {
      state.offer = action.payload;
    },
    setMaker(state, action: PayloadAction<any>) {
      state.maker = action.payload;
    },
    setStep(state, action: PayloadAction<StartSwapStep>) {
      state.step = action.payload;
    },
    reset() {
      return initialState;
    },
  },
});

export const { setBtcAmount, setRedeemAddress, setOffer, setMaker, setStep, reset } = startSwapSlice.actions;
export default startSwapSlice.reducer;