import { IconButton } from "@material-ui/core";
import FeedbackIcon from "@material-ui/icons/Feedback";
import { useState } from "react";
import FeedbackDialog from "../../feedback/FeedbackDialog";

export default function FeedbackSubmitBadge() {
  const [showFeedbackDialog, setShowFeedbackDialog] = useState(false);

  return (
    <>
      {showFeedbackDialog && (
        <FeedbackDialog
          open={showFeedbackDialog}
          onClose={() => setShowFeedbackDialog(false)}
        />
      )}
      <IconButton onClick={() => setShowFeedbackDialog(true)}>
        <FeedbackIcon />
      </IconButton>
    </>
  );
}
