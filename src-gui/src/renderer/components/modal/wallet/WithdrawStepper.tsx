import { Step, StepLabel, Stepper } from "@material-ui/core";

function getActiveStep(isPending: boolean, withdrawTxId: string | null) {
  if (isPending) {
    return 1;
  }
  if (withdrawTxId !== null) {
    return 2;
  }
  return 0;
}

export default function WithdrawStepper({
  isPending,
  withdrawTxId,
}: {
  isPending: boolean;
  withdrawTxId: string | null;
}) {
  return (
    <Stepper activeStep={getActiveStep(isPending, withdrawTxId)}>
      <Step key={0}>
        <StepLabel>Enter withdraw address</StepLabel>
      </Step>
      <Step key={2}>
        <StepLabel error={false}>Transfer funds to wallet</StepLabel>
      </Step>
    </Stepper>
  );
}
