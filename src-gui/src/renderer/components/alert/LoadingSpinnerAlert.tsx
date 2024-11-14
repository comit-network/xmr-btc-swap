import { CircularProgress } from "@material-ui/core";
import { AlertProps, Alert } from "@material-ui/lab";

export function LoadingSpinnerAlert({ ...rest }: AlertProps) {
    return <Alert icon={<CircularProgress size={22} />} {...rest} />;
}
