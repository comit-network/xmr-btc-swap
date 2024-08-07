import { configureStore } from '@reduxjs/toolkit';
import { reducers } from 'store/combinedReducer';

export const store = configureStore({
  reducer: reducers,
});

export type AppDispatch = typeof store.dispatch;
export type RootState = ReturnType<typeof store.getState>;
