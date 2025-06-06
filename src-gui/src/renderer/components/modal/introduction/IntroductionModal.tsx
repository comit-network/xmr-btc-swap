import { Modal } from "@mui/material";
import { useState } from "react";
import Slide01_GettingStarted from "./slides/Slide01_GettingStarted";
import Slide02_ChooseAMaker from "./slides/Slide02_ChooseAMaker";
import Slide03_PrepareSwap from "./slides/Slide03_PrepareSwap";
import Slide04_ExecuteSwap from "./slides/Slide04_ExecuteSwap";
import Slide05_KeepAnEyeOnYourSwaps from "./slides/Slide05_KeepAnEyeOnYourSwaps";
import Slide06_FiatPricePreference from "./slides/Slide06_FiatPricePreference";
import Slide07_ReachOut from "./slides/Slide07_ReachOut";
import {
  setFetchFiatPrices,
  setUserHasSeenIntroduction,
} from "store/features/settingsSlice";
import { useAppDispatch, useSettings } from "store/hooks";

export default function IntroductionModal() {
  const userHasSeenIntroduction = useSettings((s) => s.userHasSeenIntroduction);

  const dispatch = useAppDispatch();

  // Handle Display State
  const [open, setOpen] = useState<boolean>(!userHasSeenIntroduction);
  const [showFiat, setShowFiat] = useState<boolean>(true);
  const handleClose = () => {
    setOpen(false);
  };

  // Handle Slide Index
  const [currentSlideIndex, setCurrentSlideIndex] = useState(0);

  const handleContinue = () => {
    if (currentSlideIndex == slideComponents.length - 1) {
      handleClose();
      dispatch(setUserHasSeenIntroduction(true));
      dispatch(setFetchFiatPrices(showFiat));
      return;
    }

    setCurrentSlideIndex((i) => i + 1);
  };

  const handlePrevious = () => {
    if (currentSlideIndex == 0) {
      return;
    }

    setCurrentSlideIndex((i) => i - 1);
  };

  const slideComponents = [
    <Slide01_GettingStarted
      handleContinue={handleContinue}
      handlePrevious={handlePrevious}
      hidePreviousButton
      key="slide-01"
    />,
    <Slide02_ChooseAMaker
      handleContinue={handleContinue}
      handlePrevious={handlePrevious}
      key="slide-02"
    />,
    <Slide03_PrepareSwap
      handleContinue={handleContinue}
      handlePrevious={handlePrevious}
      key="slide-03"
    />,
    <Slide04_ExecuteSwap
      handleContinue={handleContinue}
      handlePrevious={handlePrevious}
      key="slide-04"
    />,
    <Slide05_KeepAnEyeOnYourSwaps
      handleContinue={handleContinue}
      handlePrevious={handlePrevious}
      key="slide-05"
    />,
    <Slide06_FiatPricePreference
      handleContinue={handleContinue}
      handlePrevious={handlePrevious}
      showFiat={showFiat}
      onChange={(showFiatSetting: string) =>
        setShowFiat(showFiatSetting === "show")
      }
      key="slide-06"
    />,
    <Slide07_ReachOut
      handleContinue={handleContinue}
      handlePrevious={handlePrevious}
      key="slide-07"
    />,
  ];

  return (
    <Modal
      open={open}
      onClose={handleClose}
      sx={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
      disableAutoFocus
      closeAfterTransition
    >
      {slideComponents[currentSlideIndex]}
    </Modal>
  );
}
