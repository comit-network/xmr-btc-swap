import React from 'react';
import Button, { ButtonProps } from '@material-ui/core/Button';
import CircularProgress from '@material-ui/core/CircularProgress';

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
