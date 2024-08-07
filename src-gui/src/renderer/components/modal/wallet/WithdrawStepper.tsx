import { Step, StepLabel, Stepper } from '@material-ui/core';
import { useAppSelector, useIsRpcEndpointBusy } from 'store/hooks';
import { RpcMethod } from 'models/rpcModel';

function getActiveStep(
  isWithdrawInProgress: boolean,
  withdrawTxId: string | null,
) {
  if (isWithdrawInProgress) {
    return 1;
  }
  if (withdrawTxId !== null) {
    return 2;
  }
  return 0;
}

export default function WithdrawStepper() {
  const isWithdrawInProgress = useIsRpcEndpointBusy(RpcMethod.WITHDRAW_BTC);
  const withdrawTxId = useAppSelector((s) => s.rpc.state.withdrawTxId);

  return (
    <Stepper activeStep={getActiveStep(isWithdrawInProgress, withdrawTxId)}>
      <Step key={0}>
        <StepLabel>Enter withdraw address</StepLabel>
      </Step>
      <Step key={2}>
        <StepLabel error={false}>Transfer funds to wallet</StepLabel>
      </Step>
    </Stepper>
  );
}
