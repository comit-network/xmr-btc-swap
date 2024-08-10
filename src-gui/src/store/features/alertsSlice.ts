import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { Alert } from "models/apiModel";

export interface AlertsSlice {
  alerts: Alert[];
}

const initialState: AlertsSlice = {
  alerts: [],
};

const alertsSlice = createSlice({
  name: "alerts",
  initialState,
  reducers: {
    setAlerts(slice, action: PayloadAction<Alert[]>) {
      slice.alerts = action.payload;
    },
    removeAlert(slice, action: PayloadAction<number>) {
      slice.alerts = slice.alerts.filter(
        (alert) => alert.id !== action.payload,
      );
    },
  },
});

export const { setAlerts, removeAlert } = alertsSlice.actions;
export default alertsSlice.reducer;
