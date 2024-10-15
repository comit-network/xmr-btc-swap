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
          Your input is crucial to us! We'd love to hear your thoughts on
          Atomic Swaps. We personally read every response to improve the
          project. Got two minutes to share?
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
