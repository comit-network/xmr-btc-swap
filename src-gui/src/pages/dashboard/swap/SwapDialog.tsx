import Dialog from "@mui/material/Dialog";
import DialogTitle from "@mui/material/DialogTitle";
import { Typography } from "@mui/material";
import GetBitcoin from "./getBitcoin/GetBitcoin";
import SelectMaker from "./selectMaker/SelectMaker";
import { useState } from "react";
import Offer from "./offer/Offer";

type Step = "getBitcoin" | "selectMaker" | "offer";

export default function SwapDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [currentStep, setCurrentStep] = useState<Step>("getBitcoin");

  const handleNext = () => {
    if (currentStep === "getBitcoin") {
      setCurrentStep("selectMaker");
    }
    if (currentStep === "selectMaker") {
      setCurrentStep("offer");
    }
  };

  const handleBack = () => {
    if (currentStep === "selectMaker") {
      setCurrentStep("getBitcoin");
    }
    if (currentStep === "offer") {
      setCurrentStep("selectMaker");
    }
  };

  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>
        <Typography variant="h2">Swap</Typography>
      </DialogTitle>
      {currentStep === "getBitcoin" && <GetBitcoin onNext={handleNext} />}
      {currentStep === "selectMaker" && (
        <SelectMaker onNext={handleNext} onBack={handleBack} />
      )}
      {currentStep === "offer" && (
        <Offer onNext={handleNext} onBack={handleBack} />
      )}
    </Dialog>
  );
}
