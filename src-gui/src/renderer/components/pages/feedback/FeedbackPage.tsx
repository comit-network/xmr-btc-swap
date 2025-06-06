import { Box } from "@mui/material";
import FeedbackInfoBox from "../help/FeedbackInfoBox";
import ConversationsBox from "../help/ConversationsBox";
import ContactInfoBox from "../help/ContactInfoBox";

export default function FeedbackPage() {
  return (
    <Box
      sx={{
        display: "flex",
        gap: 2,
        flexDirection: "column",
        paddingBottom: 2,
      }}
    >
      <FeedbackInfoBox />
      <ConversationsBox />
      <ContactInfoBox />
    </Box>
  );
}
