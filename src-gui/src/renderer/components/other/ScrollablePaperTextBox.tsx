import { Box, Divider, IconButton, Paper, Typography } from "@mui/material";
import FileCopyOutlinedIcon from "@mui/icons-material/FileCopyOutlined";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import { ReactNode, useEffect, useRef } from "react";
import { VList, VListHandle } from "virtua";
import { ExpandableSearchBox } from "./ExpandableSearchBox";

const MIN_HEIGHT = "10rem";

export default function ScrollablePaperTextBox({
  rows,
  title,
  copyValue,
  searchQuery = null,
  setSearchQuery = null,
  topRightButton = null,
  minHeight = MIN_HEIGHT,
  autoScroll = false,
}: {
  rows: ReactNode[];
  title: string;
  copyValue: string;
  searchQuery?: string | null;
  setSearchQuery?: ((query: string) => void) | null;
  minHeight?: string;
  topRightButton?: ReactNode | null;
  autoScroll?: boolean;
}) {
  const virtuaEl = useRef<VListHandle | null>(null);

  function onCopy() {
    navigator.clipboard.writeText(copyValue);
  }

  function scrollToBottom() {
    virtuaEl.current?.scrollToIndex(rows.length - 1);
  }

  function scrollToTop() {
    virtuaEl.current?.scrollToIndex(0);
  }

  useEffect(() => {
    if (autoScroll) {
      scrollToBottom();
    }
  }, [rows.length, autoScroll]);

  return (
    <Paper
      variant="outlined"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "0.5rem",
        padding: "0.5rem",
        width: "100%",
      }}
    >
      <Box
        style={{
          display: "flex",
          flexDirection: "row",
          justifyContent: "space-between",
          alignItems: "center",
        }}
      >
        <Typography>{title}</Typography>
        {topRightButton}
      </Box>
      <Divider />
      <Box
        style={{
          overflow: "auto",
          whiteSpace: "nowrap",
          maxHeight: minHeight,
          minHeight,
          display: "flex",
          flexDirection: "column",
          gap: "0.5rem",
        }}
      >
        <VList ref={virtuaEl} style={{ height: MIN_HEIGHT, width: "100%" }}>
          {rows}
        </VList>
      </Box>
      <Box style={{ display: "flex", gap: "0.5rem" }}>
        <IconButton onClick={onCopy} size="small">
          <FileCopyOutlinedIcon />
        </IconButton>
        <IconButton onClick={scrollToTop} size="small">
          <KeyboardArrowUpIcon />
        </IconButton>
        <IconButton onClick={scrollToBottom} size="small">
          <KeyboardArrowDownIcon />
        </IconButton>
        {searchQuery !== undefined && setSearchQuery !== undefined && (
          <ExpandableSearchBox query={searchQuery} setQuery={setSearchQuery} />
        )}
      </Box>
    </Paper>
  );
}
