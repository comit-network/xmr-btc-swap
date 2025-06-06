import Button, { ButtonProps } from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import React from "react";

interface LoadingButtonProps extends ButtonProps {
  loading: boolean;
}

const LoadingButton: React.FC<LoadingButtonProps> = ({
  loading,
  disabled,
  children,
  ...props
}) => {
  return (
    <Button
      disabled={loading || disabled}
      {...props}
      endIcon={loading && <CircularProgress size="1rem" />}
    >
      {children}
    </Button>
  );
};

export default LoadingButton;
