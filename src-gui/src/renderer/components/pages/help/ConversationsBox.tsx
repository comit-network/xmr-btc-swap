import { useState, useEffect, useMemo } from "react";
import {
  Box,
  Typography,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  Button,
  IconButton,
  TableContainer,
  Table,
  TableHead,
  TableRow,
  TableCell,
  TableBody,
  Paper,
  Badge,
  TextField,
  CircularProgress,
  InputAdornment,
  Tooltip,
  List,
  ListItem,
  ListItemIcon,
  Link,
} from "@mui/material";
import ChatIcon from "@mui/icons-material/Chat";
import SendIcon from "@mui/icons-material/Send";
import InfoBox from "renderer/components/pages/swap/swap/components/InfoBox";
import TruncatedText from "renderer/components/other/TruncatedText";
import clsx from "clsx";
import {
  useAppSelector,
  useAppDispatch,
  useUnreadMessagesCount,
} from "store/hooks";
import { markMessagesAsSeen } from "store/features/conversationsSlice";
import {
  appendFeedbackMessageViaHttp,
  fetchAllConversations,
} from "renderer/api";
import { useSnackbar } from "notistack";
import logger from "utils/logger";
import AttachmentIcon from "@mui/icons-material/Attachment";
import { Message, PrimitiveDateTimeString } from "models/apiModel";
import { formatDateTime } from "utils/conversionUtils";
import { Theme } from "renderer/components/theme";

// Hook: sorted feedback IDs by latest activity, then unread
function useSortedFeedbackIds() {
  const ids = useAppSelector((s) => s.conversations.knownFeedbackIds || []);
  const conv = useAppSelector((s) => s.conversations.conversations);
  const seen = useAppSelector((s) => new Set(s.conversations.seenMessages));
  return useMemo(() => {
    const arr = ids.map((id) => {
      const msgs = conv[id] || [];
      const unread = msgs.filter(
        (m) => m.is_from_staff && !seen.has(m.id.toString()),
      ).length;
      const latest = msgs.reduce((d, m) => {
        try {
          const formattedDate = formatDateTime(m.created_at);
          if (formattedDate.startsWith("Invalid")) return d;
          const t = new Date(formattedDate).getTime();
          return isNaN(t) ? d : Math.max(d, t);
        } catch (e) {
          return d;
        }
      }, 0);
      return { id, unread, latest };
    });
    arr.sort(
      (a, b) =>
        b.latest - a.latest || (b.unread > 0 ? 1 : 0) - (a.unread > 0 ? 1 : 0),
    );
    return arr.map((x) => x.id);
  }, [ids, conv, seen]);
}

// Main component
export default function ConversationsBox() {
  const sortedIds = useSortedFeedbackIds();
  const [openId, setOpenId] = useState<string | null>(null);

  useEffect(() => {
    // Fetch conversations via API function (handles its own dispatch)
    fetchAllConversations();
  }, []);

  return (
    <InfoBox
      title="Developer Responses"
      icon={null}
      loading={false}
      mainContent={
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            alignItems: "flex-start",
            gap: 2,
          }}
        >
          <Typography variant="subtitle2">
            View your past feedback submissions and any replies from the
            development team.
          </Typography>
          {sortedIds.length === 0 ? (
            <Typography variant="body2">No feedback submitted yet.</Typography>
          ) : (
            <TableContainer component={Paper} sx={{ maxHeight: 300 }}>
              <Table stickyHeader size="small">
                <TableHead>
                  <TableRow>
                    <TableCell
                      sx={(theme) => ({
                        width: "25%",
                        backgroundColor: theme.palette.grey[900],
                      })}
                    >
                      Last Message
                    </TableCell>
                    <TableCell
                      sx={(theme) => ({
                        width: "60%",
                        backgroundColor: theme.palette.grey[900],
                      })}
                    >
                      Preview
                    </TableCell>
                    <TableCell
                      align="right"
                      sx={(theme) => ({
                        width: "15%",
                        backgroundColor: theme.palette.grey[900],
                      })}
                    />
                  </TableRow>
                </TableHead>
                <TableBody>
                  {sortedIds.map((id) => (
                    <ConversationRow
                      key={id}
                      feedbackId={id}
                      onOpen={setOpenId}
                    />
                  ))}
                </TableBody>
              </Table>
            </TableContainer>
          )}
        </Box>
      }
      additionalContent={
        openId && (
          <ConversationModal
            open={!!openId}
            onClose={() => setOpenId(null)}
            feedbackId={openId}
          />
        )
      }
    />
  );
}

// Single row
function ConversationRow({
  feedbackId,
  onOpen,
}: {
  feedbackId: string;
  onOpen: (id: string) => void;
}) {
  const msgs = useAppSelector(
    (s) => s.conversations.conversations[feedbackId] || [],
  );
  const unread = useUnreadMessagesCount(feedbackId);
  const sorted = useMemo(
    () =>
      [...msgs].sort((a, b) => {
        try {
          const formattedDateA = formatDateTime(a.created_at);
          const formattedDateB = formatDateTime(b.created_at);
          if (formattedDateA.startsWith("Invalid")) return 1;
          if (formattedDateB.startsWith("Invalid")) return -1;
          const dateA = new Date(formattedDateA).getTime();
          const dateB = new Date(formattedDateB).getTime();
          if (isNaN(dateA)) return 1;
          if (isNaN(dateB)) return -1;
          return dateB - dateA;
        } catch (e) {
          return 0;
        }
      }),
    [msgs],
  );
  const lastMsg = sorted[0];
  const time = lastMsg ? formatDateTime(lastMsg.created_at) : "-";
  const content = lastMsg ? lastMsg.content : "No messages yet";
  const preview = (() => {
    return content;
  })();
  const hasStaff = useMemo(() => msgs.some((m) => m.is_from_staff), [msgs]);

  return (
    <TableRow>
      <TableCell style={{ width: "25%" }}>{time}</TableCell>
      <TableCell style={{ width: "60%" }}>
        "<TruncatedText limit={30}>{preview}</TruncatedText>"
      </TableCell>
      <TableCell align="right" style={{ width: "15%" }}>
        <Badge badgeContent={unread} color="primary" overlap="rectangular">
          <Tooltip
            title={
              hasStaff ? "Open Conversation" : "No developer has responded"
            }
            arrow
          >
            <span>
              <IconButton
                size="small"
                onClick={() => onOpen(feedbackId)}
                disabled={!hasStaff}
              >
                <ChatIcon />
              </IconButton>
            </span>
          </Tooltip>
        </Badge>
      </TableCell>
    </TableRow>
  );
}

// Modal
function ConversationModal({
  open,
  onClose,
  feedbackId,
}: {
  open: boolean;
  onClose: () => void;
  feedbackId: string;
}) {
  const dispatch = useAppDispatch();
  const msgs = useAppSelector(
    (s) => s.conversations.conversations[feedbackId] || [],
  );
  const [newMessage, setNewMessage] = useState("");
  const [sending, setSending] = useState(false);
  const { enqueueSnackbar } = useSnackbar();

  // Mark messages as seen when modal opens
  useEffect(() => {
    if (open && msgs.length > 0) {
      const unreadMessages = msgs.filter((m) => m.is_from_staff);
      if (unreadMessages.length > 0) {
        dispatch(markMessagesAsSeen(unreadMessages));
      }
    }
  }, [open, msgs, dispatch]);

  // Sort messages chronologically
  const sortedMsgs = useMemo(
    () =>
      [...msgs].sort((a, b) => {
        try {
          const formattedDateA = formatDateTime(a.created_at);
          const formattedDateB = formatDateTime(b.created_at);
          if (formattedDateA.startsWith("Invalid")) return -1;
          if (formattedDateB.startsWith("Invalid")) return 1;
          const dateA = new Date(formattedDateA).getTime();
          const dateB = new Date(formattedDateB).getTime();
          if (isNaN(dateA)) return -1;
          if (isNaN(dateB)) return 1;
          return dateA - dateB;
        } catch (e) {
          return 0;
        }
      }),
    [msgs],
  );

  const sendMessage = async () => {
    if (!newMessage.trim() || sending) return;
    setSending(true);
    try {
      await appendFeedbackMessageViaHttp(feedbackId, newMessage.trim());
      setNewMessage("");
      enqueueSnackbar("Message sent successfully!", { variant: "success" });
      // Fetch updated conversations
      fetchAllConversations();
    } catch (error) {
      logger.error("Error sending message:", error);
      enqueueSnackbar("Failed to send message. Please try again.", {
        variant: "error",
      });
    } finally {
      setSending(false);
    }
  };

  const handleKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle>Conversation</DialogTitle>
      <DialogContent sx={{ display: "flex", flexDirection: "column" }}>
        <Box
          sx={{
            flexGrow: 1,
            overflowY: "auto",
            display: "flex",
            flexDirection: "column",
            gap: 1,
            maxHeight: 400,
            padding: 1,
          }}
        >
          {sortedMsgs.map((msg) => (
            <MessageBubble key={msg.id} message={msg} />
          ))}
        </Box>
        <Box sx={{ flexShrink: 0, marginTop: 2 }}>
          <TextField
            fullWidth
            multiline
            rows={3}
            value={newMessage}
            onChange={(e) => setNewMessage(e.target.value)}
            onKeyPress={handleKeyPress}
            placeholder="Type your message here..."
            disabled={sending}
            InputProps={{
              endAdornment: (
                <InputAdornment position="end">
                  <IconButton
                    onClick={sendMessage}
                    disabled={!newMessage.trim() || sending}
                  >
                    {sending ? <CircularProgress size={20} /> : <SendIcon />}
                  </IconButton>
                </InputAdornment>
              ),
            }}
          />
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Close</Button>
      </DialogActions>
    </Dialog>
  );
}

// Message bubble component
function MessageBubble({ message }: { message: Message }) {
  const isStaff = message.is_from_staff;
  const time = formatDateTime(message.created_at);

  const attachments = message.attachments || [];

  return (
    <Box
      sx={{
        display: "flex",
        marginTop: 1,
        justifyContent: isStaff ? "flex-start" : "flex-end",
      }}
    >
      <Box
        sx={(theme) => ({
          padding: 1.5,
          borderRadius:
            typeof theme.shape.borderRadius === "number"
              ? theme.shape.borderRadius * 2
              : 8,
          maxWidth: "75%",
          wordBreak: "break-word",
          boxShadow: theme.shadows[1],
          ...(isStaff
            ? {
                border: `1px solid ${theme.palette.divider}`,
                color: theme.palette.text.primary,
                borderRadius: 2,
              }
            : {
                backgroundColor: theme.palette.primary.main,
                color: theme.palette.primary.contrastText,
                borderRadius: 2,
              }),
        })}
      >
        <Typography variant="body2">{message.content}</Typography>
        {attachments.length > 0 && (
          <List sx={{ marginTop: 1, marginBottom: 1, paddingLeft: 2 }}>
            {attachments.map((att, idx) => (
              <ListItem key={idx} sx={{ paddingTop: 0.5, paddingBottom: 0.5 }}>
                <ListItemIcon>
                  <AttachmentIcon />
                </ListItemIcon>
                <Link
                  href="#"
                  onClick={(e) => {
                    e.preventDefault();
                    alert(
                      `Attachment Key: ${att.key}\n\nContent:\n${att.content}`,
                    );
                  }}
                >
                  {att.key}
                </Link>
              </ListItem>
            ))}
          </List>
        )}
        <Typography
          variant="caption"
          sx={{
            marginTop: 0.5,
            fontSize: "0.75rem",
            opacity: 0.7,
            textAlign: "right",
            display: "block",
          }}
        >
          {time}
        </Typography>
      </Box>
    </Box>
  );
}
