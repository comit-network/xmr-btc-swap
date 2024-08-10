import { IconButton, Tooltip } from "@material-ui/core";
import { useAppSelector } from "store/hooks";
import TorIcon from "../../../icons/TorIcon";

export default function TorStatusBadge() {
  const tor = useAppSelector((s) => s.tor);

  if (tor.processRunning) {
    return (
      <Tooltip title="Tor is running in the background">
        <IconButton>
          <TorIcon htmlColor="#7D4698" />
        </IconButton>
      </Tooltip>
    );
  }

  return <></>;
}
