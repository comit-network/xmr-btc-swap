import { CircularProgress } from "@mui/material";
import { Alert } from "@mui/material";
import { AlertProps } from "@mui/material";

export function LoadingSpinnerAlert({ ...rest }: AlertProps) {
  return <Alert icon={<CircularProgress size={22} />} {...rest} />;
}
