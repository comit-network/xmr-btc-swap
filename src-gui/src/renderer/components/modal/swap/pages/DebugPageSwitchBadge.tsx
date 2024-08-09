import { Tooltip } from "@material-ui/core";
import IconButton from "@material-ui/core/IconButton";
import DeveloperBoardIcon from "@material-ui/icons/DeveloperBoard";

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
      >
        <DeveloperBoardIcon />
      </IconButton>
    </Tooltip>
  );
}
