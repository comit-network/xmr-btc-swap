import { Box } from "@mui/material";
import { Alert, AlertTitle } from "@mui/material";
import { removeAlert } from "store/features/alertsSlice";
import { useAppDispatch, useAppSelector } from "store/hooks";

export default function ApiAlertsBox() {
  const alerts = useAppSelector((state) => state.alerts.alerts);
  const dispatch = useAppDispatch();

  function onRemoveAlert(id: number) {
    dispatch(removeAlert(id));
  }

  if (alerts.length === 0) return null;

  return (
    <Box style={{ display: "flex", justifyContent: "center", gap: "1rem" }}>
      {alerts.map((alert) => (
        <Alert
          variant="filled"
          severity={alert.severity}
          key={alert.id}
          onClose={() => onRemoveAlert(alert.id)}
        >
          <AlertTitle>{alert.title}</AlertTitle>
          {alert.body}
        </Alert>
      ))}
    </Box>
  );
}
