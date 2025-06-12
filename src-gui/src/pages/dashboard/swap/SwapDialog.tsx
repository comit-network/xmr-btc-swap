import Dialog from "@mui/material/Dialog";
import DialogTitle from "@mui/material/DialogTitle";
import { Typography } from "@mui/material";
import GetBitcoin from "./getBitcoin/GetBitcoin";
import SelectMaker from "./selectMaker/SelectMaker";
import Offer from "./offer/Offer";
import { useAppDispatch, useAppSelector } from "store/hooks";
import { setStep, StartSwapStep } from "store/features/startSwapSlice";

export default function SwapDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const step = useAppSelector((state) => state.startSwap.step);
  const dispatch = useAppDispatch();
  const handleNext = () => {
    if (step === StartSwapStep.DepositBitcoin) {
      dispatch(setStep(StartSwapStep.SelectMaker));
    } else if (step === StartSwapStep.SelectMaker) {
      dispatch(setStep(StartSwapStep.ReviewOffer));
    }
  };

  const handleBack = () => {
    if (step === StartSwapStep.SelectMaker) {
      dispatch(setStep(StartSwapStep.DepositBitcoin));
    } else if (step === StartSwapStep.ReviewOffer) {
      dispatch(setStep(StartSwapStep.SelectMaker));
    }
  };

  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>
        <Typography variant="h2">Swap</Typography>
      </DialogTitle>
      {step === StartSwapStep.DepositBitcoin && <GetBitcoin onNext={handleNext} />}
      {step === StartSwapStep.SelectMaker && (
        <SelectMaker onNext={handleNext} onBack={handleBack} />
      )}
      {step === StartSwapStep.ReviewOffer && (
        <Offer onNext={handleNext} onBack={handleBack} />
      )}
    </Dialog>
  );
}
