import { Box, Chip, Typography } from "@material-ui/core";
import { CliLog } from "models/cliModel";
import { useMemo, useState } from "react";
import { logsToRawString } from "utils/parseUtils";
import ScrollablePaperTextBox from "./ScrollablePaperTextBox";

function RenderedCliLog({ log }: { log: CliLog }) {
  const { timestamp, level, fields } = log;

  const levelColorMap = {
    DEBUG: "#1976d2", // Blue
    INFO: "#388e3c", // Green
    WARN: "#fbc02d", // Yellow
    ERROR: "#d32f2f", // Red
    TRACE: "#8e24aa", // Purple
  };

  return (
    <Box>
      <Box
        style={{
          display: "flex",
          gap: "0.3rem",
          alignItems: "center",
        }}
      >
        <Chip
          label={level}
          size="small"
          style={{ backgroundColor: levelColorMap[level], color: "white" }}
        />
        <Chip label={timestamp} size="small" variant="outlined" />
        <Typography variant="subtitle2">{fields.message}</Typography>
      </Box>
      <Box
        sx={{
          paddingLeft: "1rem",
          paddingTop: "0.2rem",
          display: "flex",
          flexDirection: "column",
        }}
      >
        {Object.entries(fields).map(([key, value]) => {
          if (key !== "message") {
            return (
              <Typography variant="caption" key={key}>
                {key}: {JSON.stringify(value)}
              </Typography>
            );
          }
          return null;
        })}
      </Box>
    </Box>
  );
}

export default function CliLogsBox({
  label,
  logs,
}: {
  label: string;
  logs: (CliLog | string)[];
}) {
  const [searchQuery, setSearchQuery] = useState<string>("");

  const memoizedLogs = useMemo(() => {
    if (searchQuery.length === 0) {
      return logs;
    }
    return logs.filter((log) =>
      JSON.stringify(log).toLowerCase().includes(searchQuery.toLowerCase()),
    );
  }, [logs, searchQuery]);

  return (
    <ScrollablePaperTextBox
      title={label}
      copyValue={logsToRawString(logs)}
      searchQuery={searchQuery}
      setSearchQuery={setSearchQuery}
      rows={memoizedLogs.map((log) =>
        typeof log === "string" ? (
          <Typography component="pre">{log}</Typography>
        ) : (
          <RenderedCliLog log={log} key={JSON.stringify(log)} />
        ),
      )}
    />
  );
}
