import { Tooltip } from "@mui/material";
import IconButton from "@mui/material/IconButton";
import DeveloperBoardIcon from "@mui/icons-material/DeveloperBoard";

export default function DebugPageSwitchBadge({
  enabled,
  setEnabled,
}: {
  enabled: boolean;
  setEnabled: (enabled: boolean) => void;
}) {
  const handleToggle = () => {
    setEnabled(!enabled);
  };

  return (
    <Tooltip title={enabled ? "Hide debug view" : "Show debug view"}>
      <IconButton
        onClick={handleToggle}
        color={enabled ? "primary" : "default"}
        size="large"
      >
        <DeveloperBoardIcon />
      </IconButton>
    </Tooltip>
  );
}
