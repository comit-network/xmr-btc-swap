import { IconButton } from "@mui/material";
import FeedbackIcon from "@mui/icons-material/Feedback";
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
      <IconButton onClick={() => setShowFeedbackDialog(true)} size="large">
        <FeedbackIcon />
      </IconButton>
    </>
  );
}
