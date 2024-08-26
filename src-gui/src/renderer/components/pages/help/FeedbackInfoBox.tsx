import { Button, Typography } from "@material-ui/core";
import { useState } from "react";
import FeedbackDialog from "../../modal/feedback/FeedbackDialog";
import InfoBox from "../../modal/swap/InfoBox";

export default function FeedbackInfoBox() {
  const [showDialog, setShowDialog] = useState(false);

  return (
    <InfoBox
      title="Feedback"
      mainContent={
        <Typography variant="subtitle2">
          The main goal of this project is to make Atomic Swaps easier to use,
          and for that we need genuine users&apos; input. Please leave some
          feedback, it takes just two minutes. I&apos;ll read each and every
          survey response and take your feedback into consideration.
        </Typography>
      }
      additionalContent={
        <>
          <Button variant="outlined" onClick={() => setShowDialog(true)}>
            Give feedback
          </Button>
          <FeedbackDialog
            open={showDialog}
            onClose={() => setShowDialog(false)}
          />
        </>
      }
      icon={null}
      loading={false}
    />
  );
}
